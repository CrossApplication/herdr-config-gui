//! Slint prototype: the same schema and config layers as the shipping app,
//! with the UI rebuilt in Slint so the two can be compared side by side.
//!
//! Deliberately read-only. Edits live in memory and "差分を確認" shows what
//! would be written; nothing touches config.toml, so running this can never
//! disturb a real herdr setup.

// The core modules are shared with the Tauri build rather than copied. None of
// them depend on tauri, so they compile unchanged here.
#[path = "../../src-tauri/src/check.rs"]
mod check;
#[path = "../../src-tauri/src/config.rs"]
mod config;
#[path = "../../src-tauri/src/herdr.rs"]
mod herdr;
#[path = "../../src-tauri/src/overlay.rs"]
mod overlay;
#[path = "../../src-tauri/src/schema.rs"]
mod schema;

/// Colour notations and HSV, for the picker.
mod color;

/// Keybinding syntax from a Slint key event.
mod keys;

/// The physical key winit knows and Slint discards.
mod physical;

/// The rows editor's own logic, which has no counterpart in the Tauri build:
/// there it lives in TypeScript as `src/rows.ts`.
mod rows;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

slint::include_modules!();

/// What the open picker will write back to.
#[derive(Clone, Debug)]
enum PickerTarget {
    /// A colour setting such as `theme.custom.accent`.
    Item(String),
    /// A sidebar row token's `fg`, which accepts hex only.
    RowFg(String, usize, usize),
}

#[derive(Clone, Debug)]
struct Picker {
    target: PickerTarget,
    hue: f32,
    sat: f32,
    val: f32,
    /// Verbatim text, so names and `rgb()` survive being typed.
    text: String,
}

/// A capture in progress. Two phases, because Esc and Enter are bindable:
/// while `recording` every keypress becomes a chord; afterwards Esc cancels
/// and Enter confirms.
#[derive(Clone, Debug)]
struct Capture {
    path: String,
    kind: String,
    /// Set once the configured prefix key has been pressed.
    prefix_armed: bool,
    recording: bool,
    chord: String,
}

/// Sidebar bounds. The lower one keeps the handle on screen; the upper one
/// stops the sidebar swallowing the pane it is meant to sit beside.
const SIDEBAR_MIN: f32 = 150.0;
const SIDEBAR_MAX: f32 = 560.0;
const SIDEBAR_DEFAULT: f32 = 250.0;

fn clamp_sidebar(width: f32) -> f32 {
    width.clamp(SIDEBAR_MIN, SIDEBAR_MAX)
}

/// The palette the app itself uses, offered as presets.
const PRESETS: &[&str] = &[
    "#11111b", "#181825", "#1e1e2e", "#313244", "#45475a", "#7f849c", "#cdd6f4", "#89b4fa",
    "#a6e3a1", "#f9e2af", "#f38ba8", "#cba6f7",
];

/// Everything the window needs, kept on the Rust side.
struct State {
    schema: schema::Schema,
    /// Values on disk, as verbatim TOML text.
    saved: BTreeMap<String, String>,
    /// Touched settings; `None` means "back to the default".
    edits: BTreeMap<String, Option<String>>,
    selected: usize,
    /// Free-text filter across every section; empty shows one section.
    filter: String,
    picker: Option<Picker>,
    capture: Option<Capture>,
}

impl State {
    fn effective(&self, path: &str) -> Option<String> {
        match self.edits.get(path) {
            Some(v) => v.clone(),
            None => self.saved.get(path).cloned(),
        }
    }

    /// Three states, as in the shipping build: absent means the default is
    /// inherited, `""` means herdr's own off switch, anything else is set.
    fn state_label(&self, path: &str) -> &'static str {
        if self.is_dirty(path) {
            return "未保存";
        }
        match self.effective(path).as_deref() {
            None => "既定",
            Some("\"\"") => "無効",
            Some(_) => "設定済み",
        }
    }

    /// Everything a save would send.
    fn edits(&self) -> Vec<config::Edit> {
        self.edits
            .iter()
            .filter(|(p, _)| self.is_dirty(p))
            .map(|(path, value)| config::Edit {
                path: path.clone(),
                value: value.clone(),
                op: config::Op::Set,
            })
            .collect()
    }

    fn is_dirty(&self, path: &str) -> bool {
        match self.edits.get(path) {
            Some(v) => v.as_deref() != self.saved.get(path).map(|s| s.as_str()),
            None => false,
        }
    }

    /// Record an edit, dropping it when it matches what is on disk.
    fn set(&mut self, path: &str, value: Option<String>) {
        if value == self.saved.get(path).cloned() {
            self.edits.remove(path);
        } else {
            self.edits.insert(path.to_string(), value);
        }
    }

    fn item(&self, path: &str) -> Option<&schema::Item> {
        self.schema
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .find(|i| i.path == path)
    }

    /// Structured rows for a setting, falling back to herdr's default.
    fn rows_of(&self, path: &str) -> rows::Rows {
        let text = self
            .effective(path)
            .or_else(|| self.item(path).map(|i| i.default.clone()))
            .unwrap_or_default();
        rows::parse(&text).unwrap_or_default()
    }

    /// Open the picker on a setting, seeded from whatever it holds now.
    fn open_picker(&mut self, target: PickerTarget) {
        let text = match &target {
            PickerTarget::Item(path) => self
                .effective(path)
                .map(|v| display(&v))
                .or_else(|| self.item(path).map(|i| display(&i.default)))
                .unwrap_or_default(),
            PickerTarget::RowFg(path, r, i) => self
                .rows_of(path)
                .get(*r)
                .and_then(|row| row.get(*i))
                .and_then(|t| t.fg.clone())
                .unwrap_or_else(|| "#cdd6f4".into()),
        };
        let (h, sa, v) = color::to_rgb(&text)
            .map(|(r, g, b)| color::rgb_to_hsv(r, g, b))
            .unwrap_or((0.0, 0.0, 0.8));
        self.picker = Some(Picker { target, hue: h, sat: sa, val: v, text });
    }

    /// Write the picker's current text back to whatever it was opened on.
    fn commit_picker(&mut self) {
        let Some(p) = self.picker.clone() else { return };
        match &p.target {
            PickerTarget::Item(path) => {
                let value = (!p.text.trim().is_empty()).then(|| format!("{:?}", p.text));
                self.set(path, value);
            }
            PickerTarget::RowFg(path, r, i) => {
                let mut rs = self.rows_of(path);
                if let Some(cell) = rs.get_mut(*r).and_then(|row| row.get_mut(*i)) {
                    cell.fg = Some(p.text.clone());
                    let next = rows::to_toml(&rs);
                    self.set(path, Some(next));
                }
            }
        }
    }

    /// Move the picker to an HSV point and keep the text in step.
    fn picker_hsv(&mut self, hue: f32, sat: f32, val: f32) {
        if let Some(p) = &mut self.picker {
            p.hue = hue.clamp(0.0, 1.0);
            p.sat = sat.clamp(0.0, 1.0);
            p.val = val.clamp(0.0, 1.0);
            let (r, g, b) = color::hsv_to_rgb(p.hue, p.sat, p.val);
            p.text = color::to_hex(r, g, b);
        }
        self.commit_picker();
    }

    fn picker_text(&mut self, text: String) {
        if let Some(p) = &mut self.picker {
            p.text = text;
            // Follow the text when it names a colour we can locate.
            if let Some((r, g, b)) = color::to_rgb(&p.text) {
                let (h, s, v) = color::rgb_to_hsv(r, g, b);
                p.hue = h;
                p.sat = s;
                p.val = v;
            }
        }
        self.commit_picker();
    }

    /// The prefix chord in effect, needed to fold `prefix+X`.
    fn prefix_chord(&self) -> String {
        self.effective("keys.prefix")
            .or_else(|| self.item("keys.prefix").map(|i| i.default.clone()))
            .map(|v| display(&v))
            .unwrap_or_default()
    }

    fn dirty_count(&self) -> usize {
        self.schema
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .filter(|i| self.is_dirty(&i.path))
            .count()
    }
}

/// TOML text -> what the field shows. Strings lose their quotes.
fn display(text: &str) -> String {
    let t = text.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

/// Field text -> TOML, following the setting's type.
fn to_toml(text: &str, ty: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    Some(match ty {
        "string" => format!("{:?}", text),
        _ => text.trim().to_string(),
    })
}

fn kind_of(item: &schema::Item) -> &'static str {
    if item.token_set.is_some() {
        "rows"
    } else if item.color {
        "color"
    } else if item.ty == "bool" {
        "bool"
    } else if item.ty == "integer" {
        "int"
    } else {
        "text"
    }
}

/// `#rrggbb` / `#rgb` -> a Slint colour, for the swatch.
fn parse_hex(text: &str) -> Option<slint::Color> {
    let t = display(text);
    let hex = t.strip_prefix('#')?;
    let (r, g, b) = match hex.len() {
        3 => {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 1].repeat(2), 16).ok();
            (d(0)?, d(1)?, d(2)?)
        }
        6 => {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            (d(0)?, d(2)?, d(4)?)
        }
        _ => return None,
    };
    Some(slint::Color::from_rgb_u8(r, g, b))
}

/// Structured rows for a setting, or an empty model when it holds none.
fn row_entries(state: &State, item: &schema::Item) -> ModelRc<RowEntry> {
    let text = state
        .effective(&item.path)
        .unwrap_or_else(|| item.default.clone());
    let parsed = rows::parse(&text).unwrap_or_default();
    let entries: Vec<RowEntry> = parsed
        .iter()
        .map(|row| RowEntry {
            preview: if row.is_empty() {
                "(空の行)".into()
            } else {
                row.iter()
                    .map(|t| t.token.clone())
                    .collect::<Vec<_>>()
                    .join("  ")
                    .into()
            },
            tokens: ModelRc::new(VecModel::from(
                row.iter()
                    .map(|t| TokenCell {
                        token: t.token.clone().into(),
                        fg: t.fg.as_deref().and_then(parse_hex).unwrap_or(
                            slint::Color::from_rgb_u8(205, 214, 244),
                        ),
                        has_fg: t.fg.is_some(),
                        bold: t.bold.unwrap_or(false),
                        dim: t.dim.unwrap_or(false),
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    ModelRc::new(VecModel::from(entries))
}

/// The token family the shown section's rows take, for the token dropdowns.
fn allowed_tokens(state: &State) -> ModelRc<SharedString> {
    let set = state
        .schema
        .sections
        .get(state.selected)
        .and_then(|s| s.items.iter().find_map(|i| i.token_set));
    let mut names: Vec<SharedString> = match set {
        Some("space") => overlay::SPACE_ROW_TOKENS.iter().map(|s| (*s).into()).collect(),
        Some(_) => overlay::AGENT_ROW_TOKENS.iter().map(|s| (*s).into()).collect(),
        None => Vec::new(),
    };
    if !names.is_empty() {
        names.push(rows::CUSTOM_OPTION.into());
    }
    ModelRc::new(VecModel::from(names))
}

fn section_rows(state: &State) -> ModelRc<SectionRow> {
    let rows: Vec<SectionRow> = state
        .schema
        .sections
        .iter()
        .map(|s| SectionRow {
            name: s.name.clone().into(),
            total: s.items.len() as i32,
            set_count: s
                .items
                .iter()
                .filter(|i| state.effective(&i.path).is_some())
                .count() as i32,
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Items to show: one section, or every match while filtering.
fn visible_items(state: &State) -> Vec<&schema::Item> {
    let needle = state.filter.trim().to_lowercase();
    if needle.is_empty() {
        return state
            .schema
            .sections
            .get(state.selected)
            .map(|s| s.items.iter().collect())
            .unwrap_or_default();
    }
    state
        .schema
        .sections
        .iter()
        .flat_map(|s| s.items.iter())
        .filter(|i| {
            i.path.to_lowercase().contains(&needle)
                || i.doc.join(" ").to_lowercase().contains(&needle)
                || i.trailing.to_lowercase().contains(&needle)
        })
        .collect()
}

fn item_rows(state: &State) -> ModelRc<ItemRow> {
    let filtering = !state.filter.trim().is_empty();
    let rows: Vec<ItemRow> = visible_items(state)
        .into_iter()
        .map(|item| {
            let value = state.effective(&item.path);
            let swatch = value
                .as_deref()
                .or(Some(item.default.as_str()))
                .and_then(parse_hex);
            ItemRow {
                path: item.path.clone().into(),
                key: item.key.clone().into(),
                doc: [item.doc.join(" "), item.trailing.clone()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .into(),
                kind: kind_of(item).into(),
                value: value.as_deref().map(display).unwrap_or_default().into(),
                default_text: display(&item.default).into(),
                is_set: value.is_some(),
                is_dirty: state.is_dirty(&item.path),
                swatch: swatch.unwrap_or(slint::Color::from_rgb_u8(30, 30, 46)),
                has_swatch: swatch.is_some(),
                rows: row_entries(state, item),
                is_binding: item.binding_kind.is_some(),
                // A hit can come from any section, so the key alone would not
                // say where it lives.
                label: if filtering {
                    item.path.clone()
                } else {
                    item.key.clone()
                }
                .into(),
                state_label: state.state_label(&item.path).into(),
                empty_disables: item.empty_disables,
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn refresh_picker(app: &App, state: &State) {
    let Some(p) = &state.picker else {
        app.set_picker_open(false);
        return;
    };
    let (hr, hg, hb) = color::hsv_to_rgb(p.hue, 1.0, 1.0);
    let preview = color::to_rgb(&p.text);
    let allow_reset = matches!(&p.target, PickerTarget::Item(_));
    let note = match color::form(&p.text) {
        color::Form::Malformed => "色として解釈できません".to_string(),
        color::Form::Name if !allow_reset => "行のスタイルは #rgb / #rrggbb のみ受け付けます".into(),
        color::Form::Rgb if !allow_reset => "行のスタイルは #rgb / #rrggbb のみ受け付けます".into(),
        color::Form::Name => "名前付き色（herdr は検証しません）".into(),
        _ => String::new(),
    };

    app.set_picker_open(true);
    app.set_picker_title(
        match &p.target {
            PickerTarget::Item(path) => path.clone(),
            PickerTarget::RowFg(path, r, i) => format!("{path} — {} 行目 {} 番目 の fg", r + 1, i + 1),
        }
        .into(),
    );
    app.set_picker_hue_color(slint::Color::from_rgb_u8(hr, hg, hb));
    app.set_picker_current(
        preview
            .map(|(r, g, b)| slint::Color::from_rgb_u8(r, g, b))
            .unwrap_or(slint::Color::from_rgb_u8(30, 30, 46)),
    );
    app.set_picker_hex(p.text.clone().into());
    app.set_picker_note(note.into());
    app.set_picker_hue(p.hue);
    app.set_picker_sat(p.sat);
    app.set_picker_val(p.val);
    app.set_picker_allow_reset(allow_reset);
    app.set_picker_presets(ModelRc::new(VecModel::from(
        PRESETS
            .iter()
            .filter_map(|h| color::parse_hex(h))
            .map(|(r, g, b)| slint::Color::from_rgb_u8(r, g, b))
            .collect::<Vec<_>>(),
    )));
}

fn refresh_capture(app: &App, state: &State) {
    let Some(c) = &state.capture else {
        app.set_capture_open(false);
        return;
    };
    let uses_prefix = c.kind == "action" || c.kind == "command";
    let prefix = state.prefix_chord();
    let hint = if c.recording {
        match c.kind.as_str() {
            "prefix" => "新しい prefix キーを押してください（prefix+ は付きません）".to_string(),
            "navigate" => "navigate モード中に使う単独キーを押してください".to_string(),
            _ if uses_prefix && !prefix.is_empty() => format!(
                "キーを押してください。先に prefix ({prefix}) を押すと prefix モードのバインドになります。"
            ),
            _ => "キーを押してください。".to_string(),
        }
    } else {
        "Esc でキャンセル、Enter で確定。もう一度録音もできます。".to_string()
    };

    let (level, note) = if c.chord.is_empty() {
        ("", String::new())
    } else {
        let (lvl, reason) = keys::risk(&c.chord, &c.kind);
        (lvl, reason.to_string())
    };

    app.set_capture_open(true);
    app.set_capture_title(format!("{} ({})", c.path, c.kind).into());
    app.set_capture_hint(hint.into());
    app.set_capture_chord(
        if c.prefix_armed && c.chord.is_empty() {
            format!("{prefix} + …")
        } else {
            c.chord.clone()
        }
        .into(),
    );
    app.set_capture_note(note.into());
    app.set_capture_level(level.into());
    app.set_capture_recording(c.recording);
}

fn section_title(state: &State) -> String {
    let needle = state.filter.trim();
    if !needle.is_empty() {
        return format!("検索: \"{needle}\" — {} 件", visible_items(state).len());
    }
    match state.schema.sections.get(state.selected) {
        Some(s) if s.array_of_tables => format!("[[{}]]", s.name),
        Some(s) if s.name.is_empty() => "(root)".into(),
        Some(s) => format!("[{}]", s.name),
        None => String::new(),
    }
}

fn refresh(app: &App, state: &State) {
    app.set_sections(section_rows(state));
    app.set_items(item_rows(state));
    app.set_section_title(section_title(state).into());
    app.set_selected(state.selected as i32);
    app.set_dirty_count(state.dirty_count() as i32);
    app.set_allowed_tokens(allowed_tokens(state));
    app.set_filter(state.filter.clone().into());
    refresh_picker(app, state);
    refresh_capture(app, state);
}

fn main() -> Result<(), slint::PlatformError> {
    let default_config = herdr::default_config().unwrap_or_else(|e| {
        eprintln!("herdr --default-config failed: {e}");
        String::new()
    });
    let parsed = schema::build(&default_config);
    let cfg = config::load();

    let meta = format!(
        "{} · {} 項目 · {}{}",
        herdr::version().unwrap_or_else(|| "herdr が見つかりません".into()),
        parsed.item_count,
        cfg.path,
        if cfg.exists { "" } else { " (未作成)" }
    );

    let state = Rc::new(RefCell::new(State {
        schema: parsed,
        saved: cfg.values.into_iter().collect(),
        edits: BTreeMap::new(),
        selected: 0,
        filter: String::new(),
        picker: None,
        capture: None,
    }));

    // Must be selected before any window exists.
    slint::BackendSelector::new()
        .with_winit_custom_application_handler(physical::PhysicalKeyRecorder)
        .select()
        .map_err(|e| slint::PlatformError::Other(format!("backend selection failed: {e}")))?;

    let app = App::new()?;
    app.set_meta(meta.into());
    app.set_result("".into());
    refresh(&app, &state.borrow());

    let types: BTreeMap<String, String> = state
        .borrow()
        .schema
        .sections
        .iter()
        .flat_map(|s| s.items.iter())
        .map(|i| (i.path.clone(), i.ty.clone()))
        .collect();

    {
        let weak = app.as_weak();
        app.on_resize_sidebar(move |delta| {
            let app = weak.unwrap();
            app.set_sidebar_width(clamp_sidebar(app.get_sidebar_width() + delta));
        });
    }
    {
        let weak = app.as_weak();
        app.on_reset_sidebar(move || {
            weak.unwrap().set_sidebar_width(SIDEBAR_DEFAULT);
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_filter_changed(move |text| {
            state.borrow_mut().filter = text.to_string();
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_select_section(move |index| {
            let mut s = state.borrow_mut();
            s.selected = index.max(0) as usize;
            // Picking a section is also how you leave a search.
            s.filter.clear();
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_edited(move |path, value| {
            let path = path.to_string();
            let ty = types.get(&path).cloned().unwrap_or_else(|| "string".into());
            let next = to_toml(&value, &ty);
            state.borrow_mut().set(&path, next);
            let app = weak.unwrap();
            let s = state.borrow();
            app.set_sections(section_rows(&s));
            app.set_dirty_count(s.dirty_count() as i32);
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_rows_act(move |path, action, row, index, delta| {
            let path = path.to_string();
            let (r, i, d) = (row.max(0) as usize, index.max(0) as usize, delta as isize);
            let mut s = state.borrow_mut();
            let mut rs = s.rows_of(&path);
            let first_token = match s.item(&path).and_then(|it| it.token_set) {
                Some("space") => overlay::SPACE_ROW_TOKENS[0],
                _ => overlay::AGENT_ROW_TOKENS[0],
            };
            match action.as_str() {
                "add-row" => rs.push(Vec::new()),
                "del-row" if r < rs.len() => {
                    rs.remove(r);
                }
                "move-row" => rows::move_row(&mut rs, r, d),
                "add-token" if r < rs.len() => rs[r].push(rows::TokenSpec {
                    token: first_token.to_string(),
                    ..Default::default()
                }),
                "del-token" if r < rs.len() && i < rs[r].len() => {
                    rs[r].remove(i);
                }
                "move-token" if r < rs.len() && i < rs[r].len() => {
                    rows::move_token(&mut rs, r, i, d)
                }
                _ => return,
            }
            let next = rows::to_toml(&rs);
            s.set(&path, Some(next));
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_rows_set_token(move |path, row, index, value| {
            let path = path.to_string();
            let (r, i) = (row.max(0) as usize, index.max(0) as usize);
            let mut s = state.borrow_mut();
            let mut rs = s.rows_of(&path);
            if r >= rs.len() || i >= rs[r].len() {
                return;
            }
            // Choosing "custom" seeds the sigil so the name has a start.
            rs[r][i].token = if value == rows::CUSTOM_OPTION {
                "$".to_string()
            } else {
                value.to_string()
            };
            let next = rows::to_toml(&rs);
            s.set(&path, Some(next));
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_rows_set_flag(move |path, row, index, field, on| {
            let path = path.to_string();
            let (r, i) = (row.max(0) as usize, index.max(0) as usize);
            let mut s = state.borrow_mut();
            let mut rs = s.rows_of(&path);
            if r >= rs.len() || i >= rs[r].len() {
                return;
            }
            // An omitted field means "keep the contextual default", which is
            // not the same as writing false, so flags are removed when off.
            let cell = &mut rs[r][i];
            match field.as_str() {
                "fg-on" => cell.fg = on.then(|| "#cdd6f4".to_string()),
                "bold" => cell.bold = on.then_some(true),
                "dim" => cell.dim = on.then_some(true),
                _ => return,
            }
            let next = rows::to_toml(&rs);
            s.set(&path, Some(next));
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_swatch_clicked(move |path| {
            state
                .borrow_mut()
                .open_picker(PickerTarget::Item(path.to_string()));
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_record(move |path| {
            let mut s = state.borrow_mut();
            let kind = s
                .item(&path)
                .and_then(|i| i.binding_kind)
                .unwrap_or("action")
                .to_string();
            s.capture = Some(Capture {
                path: path.to_string(),
                kind,
                prefix_armed: false,
                recording: true,
                chord: String::new(),
            });
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_capture_pressed(move |key, ctrl, shift, alt, meta| {
            // Raw record of what Slint hands us, so real hardware presses can
            // be compared against what the browser build sees.
            eprintln!(
                "[key] slint={key:?} physical={:?} codepoints=[{}] ctrl={ctrl} shift={shift} alt={alt} meta={meta}",
                physical::last(),
                key.chars()
                    .map(|c| format!("U+{:04X}", c as u32))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let mut s = state.borrow_mut();
            let prefix = s.prefix_chord();
            let Some(c) = s.capture.as_mut() else { return };

            if !c.recording {
                // Esc and Enter only act once a chord has been captured, so
                // they remain bindable themselves.
                match key.as_str() {
                    "esc" => s.capture = None,
                    "enter" => {
                        let chord = c.chord.clone();
                        let path = c.path.clone();
                        s.capture = None;
                        if !chord.is_empty() {
                            s.set(&path, Some(format!("{chord:?}")));
                        }
                    }
                    _ => {}
                }
                drop(s);
                let app = weak.unwrap();
                refresh(&app, &state.borrow());
                return;
            }

            let raw = keys::RawKey {
                key: key.to_string(),
                // Recorded by the winit handler just before Slint delivered
                // this same press.
                physical: physical::last(),
                ctrl,
                shift,
                alt,
                meta,
            };
            let Some(chord) = keys::from_event(&raw, cfg!(target_os = "macos")) else {
                return; // modifiers only: keep waiting
            };

            let uses_prefix = c.kind == "action" || c.kind == "command";
            if uses_prefix && !c.prefix_armed && !prefix.is_empty() && chord == prefix {
                c.prefix_armed = true;
            } else {
                c.chord = if c.prefix_armed {
                    keys::with_prefix(&chord)
                } else {
                    chord
                };
                c.recording = false;
            }
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_capture_again(move || {
            if let Some(c) = state.borrow_mut().capture.as_mut() {
                c.recording = true;
                c.prefix_armed = false;
                c.chord.clear();
            }
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_capture_confirm(move || {
            let mut s = state.borrow_mut();
            if let Some(c) = s.capture.clone() {
                s.capture = None;
                if !c.chord.is_empty() {
                    s.set(&c.path, Some(format!("{:?}", c.chord)));
                }
            }
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_capture_clear(move || {
            let mut s = state.borrow_mut();
            if let Some(c) = s.capture.clone() {
                s.capture = None;
                // herdr's own way of unbinding an action.
                s.set(&c.path, Some("\"\"".into()));
            }
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_capture_cancel(move || {
            state.borrow_mut().capture = None;
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_rows_open_fg(move |path, row, index| {
            state.borrow_mut().open_picker(PickerTarget::RowFg(
                path.to_string(),
                row.max(0) as usize,
                index.max(0) as usize,
            ));
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_picker_pick_sv(move |sat, val| {
            let hue = state.borrow().picker.as_ref().map_or(0.0, |p| p.hue);
            state.borrow_mut().picker_hsv(hue, sat, val);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_picker_pick_hue(move |hue| {
            let (sat, val) = state
                .borrow()
                .picker
                .as_ref()
                .map_or((1.0, 1.0), |p| (p.sat, p.val));
            state.borrow_mut().picker_hsv(hue, sat, val);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_picker_set_hex(move |text| {
            state.borrow_mut().picker_text(text.to_string());
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_picker_preset(move |index| {
            if let Some(hex) = PRESETS.get(index.max(0) as usize) {
                state.borrow_mut().picker_text((*hex).to_string());
            }
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_picker_reset(move || {
            // herdr's own escape hatch: `panel_bg = "reset"`.
            state.borrow_mut().picker_text("reset".into());
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_picker_close(move || {
            state.borrow_mut().picker = None;
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_revert(move || {
            state.borrow_mut().edits.clear();
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
            app.set_result("取り消しました".into());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_revert_one(move |path| {
            state.borrow_mut().set(&path, None);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_disable_one(move |path| {
            // herdr's own way of turning a setting off.
            state.borrow_mut().set(&path, Some("\"\"".into()));
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_save(move || {
            let edits = state.borrow().edits();
            let app = weak.unwrap();
            if edits.is_empty() {
                app.set_result("変更はありません".into());
                return;
            }
            // The shared writer: backup, minimal diff, pre-flight check, then
            // reload. Fatal content never reaches disk.
            let text = match config::save(edits, true) {
                Err(e) => format!("保存に失敗しました: {e}"),
                Ok(r) if !r.written => format!(
                    "保存を中止しました（編集内容は残っています）: {}",
                    r.check.raw.replace('\n', " ")
                ),
                Ok(r) => {
                    let changed = r
                        .changes
                        .iter()
                        .filter(|c| c.action != "noop")
                        .map(|c| format!("{} {}", c.action, c.path))
                        .collect::<Vec<_>>()
                        .join(" / ");
                    let reload = match r.reloaded {
                        Some(true) => "reload: ok".to_string(),
                        Some(false) => format!("reload: {}", r.reload_output),
                        None => String::new(),
                    };
                    let backup = r
                        .backup
                        .map(|b| format!("  backup: {b}"))
                        .unwrap_or_default();
                    format!("{changed}  |  {reload}{backup}")
                }
            };

            // Re-read, so what the form shows is what is on disk.
            let mut s = state.borrow_mut();
            let cfg = config::load();
            s.saved = cfg.values.into_iter().collect();
            s.edits.clear();
            drop(s);
            refresh(&app, &state.borrow());
            app.set_result(text.into());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_preview(move || {
            let s = state.borrow();
            let edits = s.edits();
            let text = if edits.is_empty() {
                "変更はありません".to_string()
            } else {
                let raw = std::fs::read_to_string(
                    config::config_path().unwrap_or_default(),
                )
                .unwrap_or_default();
                match config::apply_edits(&raw, &edits) {
                    Ok((after, changes)) => {
                        let report = check::check_toml(&after);
                        let summary = changes
                            .iter()
                            .filter(|c| c.action != "noop")
                            .map(|c| format!("{} {}", c.action, c.path))
                            .collect::<Vec<_>>()
                            .join(" / ");
                        format!(
                            "{summary}  |  herdr: {}",
                            if report.ok {
                                "問題なし".to_string()
                            } else {
                                report.raw.replace('\n', " ")
                            }
                        )
                    }
                    Err(e) => e,
                }
            };
            weak.unwrap().set_result(text.into());
        });
    }
    {
        let weak = app.as_weak();
        app.on_toggle_about(move || {
            let app = weak.unwrap();
            app.set_show_about(!app.get_show_about());
        });
    }

    if std::env::var_os("PROTO_DUMP").is_some() {
        // Evidence that the models really carry the config, for environments
        // where the window cannot be inspected.
        use slint::Model;
        let s = state.borrow();
        println!("sections      : {}", app.get_sections().row_count());
        println!("items (先頭)  : {}", app.get_items().row_count());
        println!("section title : {}", app.get_section_title());
        println!("meta          : {}", app.get_meta());
        for (i, section) in s.schema.sections.iter().enumerate().take(3) {
            println!("  [{}] {} ({} 項目)", i, section.name, section.items.len());
        }
        // Point the dump at the rows section so its shape is visible.
        if let Some(idx) = s
            .schema
            .sections
            .iter()
            .position(|x| x.name == "ui.sidebar.agents")
        {
            let section = &s.schema.sections[idx];
            for item in section.items.iter().filter(|i| i.token_set.is_some()) {
                let parsed = rows::parse(&item.default).unwrap_or_default();
                println!(
                    "rows 項目     : {} -> {} 行 / {} トークン  ({:?})",
                    item.path,
                    parsed.len(),
                    parsed.iter().map(|r| r.len()).sum::<usize>(),
                    item.token_set
                );
                println!("  再生成       : {}", rows::to_toml(&parsed));
            }
        }
        // Drive the picker without a window: open it on a real colour
        // setting, move it, and read back what would be written.
        drop(s);
        {
            let mut st = state.borrow_mut();
            st.open_picker(PickerTarget::Item("theme.custom.accent".into()));
            let p = st.picker.clone().unwrap();
            println!(
                "picker 初期    : text={} hue={:.3} sat={:.3} val={:.3}",
                p.text, p.hue, p.sat, p.val
            );
            st.picker_hsv(0.5, 1.0, 1.0);
            println!("HSV(0.5,1,1)  : {}", st.picker.as_ref().unwrap().text);
            println!("  書き込み値   : {:?}", st.effective("theme.custom.accent"));
            st.picker_text("cyan".into());
            println!("テキスト cyan  : hue={:.3} 書き込み値={:?}",
                st.picker.as_ref().unwrap().hue, st.effective("theme.custom.accent"));
            st.picker_text("reset".into());
            println!("reset          : {:?}", st.effective("theme.custom.accent"));

            st.open_picker(PickerTarget::RowFg("ui.sidebar.agents.rows".into(), 0, 0));
            st.picker_hsv(0.0, 1.0, 1.0);
            println!("行 fg          : {:?}", st.effective("ui.sidebar.agents.rows"));
        }
        let s = state.borrow();
        let colors = s
            .schema
            .sections
            .iter()
            .flat_map(|x| x.items.iter())
            .filter(|i| i.color)
            .count();
        println!("color 項目    : {colors}");
        return Ok(());
    }

    if std::env::var_os("PROTO_TRACE").is_some() {
        // Report what the window actually is, from inside the event loop.
        let weak = app.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(1500),
            move || {
                let app = weak.unwrap();
                let w = app.window();
                let size = w.size();
                println!(
                    "window: {}x{} visible={} scale={}",
                    size.width,
                    size.height,
                    w.is_visible(),
                    w.scale_factor()
                );
                slint::quit_event_loop().ok();
            },
        );
        app.run()?;
        return Ok(());
    }

    app.run()
}

#[cfg(test)]
mod sidebar_tests {
    use super::*;

    #[test]
    fn the_sidebar_stays_within_its_bounds() {
        assert_eq!(clamp_sidebar(300.0), 300.0);
        assert_eq!(clamp_sidebar(20.0), SIDEBAR_MIN, "the handle must stay reachable");
        assert_eq!(clamp_sidebar(9999.0), SIDEBAR_MAX, "the sidebar must not swallow the pane");
        assert_eq!(clamp_sidebar(SIDEBAR_DEFAULT), SIDEBAR_DEFAULT);
    }

    #[test]
    fn dragging_past_an_edge_and_back_returns_to_where_it_was() {
        // Each drag step is a delta, so clamping must not accumulate.
        let mut w = SIDEBAR_DEFAULT;
        for _ in 0..40 {
            w = clamp_sidebar(w - 50.0);
        }
        assert_eq!(w, SIDEBAR_MIN);
        for _ in 0..4 {
            w = clamp_sidebar(w + 50.0);
        }
        assert_eq!(w, SIDEBAR_MIN + 200.0);
    }
}
