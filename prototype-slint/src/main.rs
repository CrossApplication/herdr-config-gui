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
    problems_open: bool,
    diff_open: bool,
    /// Array-of-tables entries marked for deletion, e.g. `keys.command[1]`.
    removed_entries: std::collections::BTreeSet<String>,
}

/// `keys.command[0].key` -> `keys.command.key`, for schema lookups.
fn strip_index(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut depth = 0usize;
    for c in path.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The index in a path that ends in one: `keys.command[2]` -> 2.
fn entry_index_of(path: &str) -> Option<usize> {
    let rest = path.rsplit_once('[')?.1;
    rest.strip_suffix(']')?.parse().ok()
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
        let removed_prefixes: Vec<String> =
            self.removed_entries.iter().map(|p| format!("{p}.")).collect();
        let mut out: Vec<config::Edit> = self
            .edits
            .iter()
            .filter(|(p, _)| self.is_dirty(p))
            // No point writing into an entry that is about to be deleted.
            .filter(|(p, _)| !removed_prefixes.iter().any(|pre| p.starts_with(pre)))
            .map(|(path, value)| config::Edit {
                path: path.clone(),
                value: value.clone(),
                op: config::Op::Set,
            })
            .collect();
        out.extend(self.removed_entries.iter().map(|path| config::Edit {
            path: path.clone(),
            value: None,
            op: config::Op::RemoveEntry,
        }));
        out
    }

    fn has_pending(&self) -> bool {
        self.dirty_count() > 0 || !self.removed_entries.is_empty()
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

    /// Items in an array-of-tables section are templates: the schema knows
    /// `keys.command.key`, the document holds `keys.command[0].key`.
    fn item(&self, path: &str) -> Option<&schema::Item> {
        let generic = strip_index(path);
        self.schema
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .find(|i| i.path == path || i.path == generic)
    }

    /// Entry indices in use for an array-of-tables section, lowest first.
    fn entry_indices(&self, section: &str) -> Vec<usize> {
        let prefix = format!("{section}[");
        let mut found: std::collections::BTreeSet<usize> = Default::default();
        for key in self.saved.keys().chain(self.edits.keys()) {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if let Some((n, _)) = rest.split_once(']') {
                    if let Ok(i) = n.parse::<usize>() {
                        found.insert(i);
                    }
                }
            }
        }
        for removed in &self.removed_entries {
            if let Some(i) = entry_index_of(removed) {
                found.remove(&i);
            }
        }
        found.into_iter().collect()
    }

    /// Append an entry, seeded so it is a valid row rather than a blank one.
    fn add_entry(&mut self, section: &str) {
        let used = self.entry_indices(section);
        let removed: Vec<usize> = self
            .removed_entries
            .iter()
            .filter(|p| p.starts_with(&format!("{section}[")))
            .filter_map(|p| entry_index_of(p))
            .collect();
        let next = used
            .iter()
            .chain(removed.iter())
            .copied()
            .max()
            .map_or(0, |m| m + 1);
        // `type` is the one field with a safe default; herdr warns about the
        // missing command until it is filled, which is honest feedback.
        self.set(&format!("{section}[{next}].type"), Some("\"shell\"".into()));
    }

    fn remove_entry(&mut self, section: &str, index: usize) {
        let path = format!("{section}[{index}]");
        let prefix = format!("{path}.");
        self.edits.retain(|k, _| !k.starts_with(&prefix));
        // An entry that was never saved just disappears; one on disk needs an op.
        if self.saved.keys().any(|k| k.starts_with(&prefix)) {
            self.removed_entries.insert(path);
        }
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

    /// Every binding in the effective config, defaults included, which is
    /// what the user will actually be running.
    fn bindings(&self) -> Vec<keys::Entry> {
        self.schema
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .filter_map(|i| {
                i.binding_kind.map(|kind| keys::Entry {
                    path: i.path.clone(),
                    value: display(
                        &self
                            .effective(&i.path)
                            .unwrap_or_else(|| i.default.clone()),
                    ),
                    kind: kind.to_string(),
                    accepts_range: i.accepts_range,
                })
            })
            .collect()
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

/// A popup dimension carries two TOML types in one field: a percentage is a
/// string bounded to 1..100, a cell count is a bare integer. herdr rejects
/// `width = "120"` outright, so the quoting follows what was typed.
fn size_to_toml(text: &str) -> Result<Option<String>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if let Some(pct) = t.strip_suffix('%') {
        let n: u32 = pct
            .parse()
            .map_err(|_| "パーセントは 1% から 100% の範囲です".to_string())?;
        if !(1..=100).contains(&n) {
            return Err("パーセントは 1% から 100% の範囲です".into());
        }
        return Ok(Some(format!("{t:?}")));
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return Ok(Some(t.to_string())); // cells, deliberately unquoted
    }
    Err("\"80%\" のようなパーセント、またはセル数の整数を入力してください".into())
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
fn row_entries(state: &State, item: &schema::Item, path: &str) -> ModelRc<RowEntry> {
    let text = state
        .effective(path)
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

/// One row. `path` may differ from the item's own when the item is a template
/// for an array-of-tables entry.
fn item_row(state: &State, item: &schema::Item, path: &str, filtering: bool) -> ItemRow {
        let value = state.effective(path);
        let swatch = value
            .as_deref()
            .or(Some(item.default.as_str()))
            .and_then(parse_hex);
        ItemRow {
            path: path.to_string().into(),
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
            is_dirty: state.is_dirty(path),
            swatch: swatch.unwrap_or(slint::Color::from_rgb_u8(30, 30, 46)),
            has_swatch: swatch.is_some(),
            rows: row_entries(state, item, path),
            is_binding: item.binding_kind.is_some(),
            // A hit can come from any section, so the key alone would not
            // say where it lives.
            label: if filtering {
                path.to_string()
            } else {
                item.key.clone()
            }
            .into(),
            state_label: state.state_label(path).into(),
            empty_disables: item.empty_disables,
            // Only a verified set reaches the form. Prose-scraped literals are
            // examples, and the documentation line below the field already
            // lists them.
            enum_values: ModelRc::new(VecModel::from(if item.enum_strict {
                item.enum_candidates
                    .iter()
                    .map(|c| SharedString::from(c.as_str()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            })),
            enum_strict: item.enum_strict,
            from_overlay: item.from_overlay,
            is_size: item.size,
            key_note: key_note(state, item, path).0.into(),
            key_level: key_note(state, item, path).1.into(),
        }
}


fn item_rows(state: &State) -> ModelRc<ItemRow> {
    let filtering = !state.filter.trim().is_empty();
    // A list section's rows live inside its entries instead.
    if !filtering && section_is_list(state) {
        return ModelRc::new(VecModel::from(Vec::<ItemRow>::new()));
    }
    let rows: Vec<ItemRow> = visible_items(state)
        .into_iter()
        .map(|item| item_row(state, item, &item.path, filtering))
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn section_is_list(state: &State) -> bool {
    state
        .schema
        .sections
        .get(state.selected)
        .is_some_and(|s| s.array_of_tables)
}

/// One card per `[[section]]` entry, each holding the template's fields at
/// that entry's index.
fn entry_cards(state: &State) -> ModelRc<EntryCard> {
    if !section_is_list(state) || !state.filter.trim().is_empty() {
        return ModelRc::new(VecModel::from(Vec::<EntryCard>::new()));
    }
    let Some(section) = state.schema.sections.get(state.selected) else {
        return ModelRc::new(VecModel::from(Vec::<EntryCard>::new()));
    };
    let cards: Vec<EntryCard> = state
        .entry_indices(&section.name)
        .into_iter()
        .map(|index| {
            let items: Vec<ItemRow> = section
                .items
                .iter()
                .map(|tpl| {
                    let path = format!("{}[{index}].{}", section.name, tpl.key);
                    item_row(state, tpl, &path, false)
                })
                .collect();
            // Whatever identifies the entry, for the card header.
            let label = ["description", "command", "key"]
                .iter()
                .find_map(|k| {
                    state
                        .effective(&format!("{}[{index}].{k}", section.name))
                        .map(|v| display(&v))
                        .filter(|v| !v.is_empty())
                })
                .unwrap_or_else(|| "(未入力)".into());
            EntryCard {
                index: index as i32,
                label: label.into(),
                items: ModelRc::new(VecModel::from(items)),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(cards))
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

const ADVICE: &[(&str, &str)] = &[
    (
        "invalid",
        "herdr が受け付けない値です。別のキーに録音し直してください。",
    ),
    (
        "conflict",
        "同じキーに複数の動作が割り当たっているため、意図しない動作になります。どちらか一方を録音し直すか、無効にしてください。",
    ),
    (
        "risky",
        "外側の端末がこのキーを herdr まで届けない可能性があります。prefix+ を付けた形か ctrl+英字 / ファンクションキーが確実です。",
    ),
];

fn refresh_problems(app: &App, state: &State) {
    let found = keys::problems(&state.bindings());
    let errors = keys::error_count(&found);
    app.set_problem_errors(errors as i32);
    app.set_problem_warnings((found.len() - errors) as i32);
    app.set_problems_open(state.problems_open);
    app.set_diff_open(state.diff_open);

    let label = |kind: &str| match kind {
        "invalid" => "構文エラー",
        "conflict" => "キー衝突",
        _ => "端末依存",
    };
    let rows: Vec<ProblemRow> = found
        .iter()
        .map(|p| ProblemRow {
            label: label(p.kind).into(),
            severity: p.severity.into(),
            chord: p.chord.clone().into(),
            detail: p.detail.clone().into(),
            advice: ADVICE
                .iter()
                .find(|(k, _)| *k == p.kind)
                .map(|(_, a)| *a)
                .unwrap_or("")
                .into(),
            scope: p.scope.unwrap_or("").into(),
            entries: ModelRc::new(VecModel::from(
                p.paths
                    .iter()
                    .map(|path| ProblemEntry {
                        path: path.clone().into(),
                        value: state
                            .effective(path)
                            .map(|v| display(&v))
                            .unwrap_or_default()
                            .into(),
                        state: state.state_label(path).into(),
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    app.set_problems(ModelRc::new(VecModel::from(rows)));
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
        let r = keys::risk(&c.chord, &c.kind);
        (r.level, r.reason.to_string())
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

/// Terminal-reliability note for a binding, or a validation error when the
/// value is one herdr would reject.
fn key_note(state: &State, item: &schema::Item, path: &str) -> (String, &'static str) {
    let Some(kind) = item.binding_kind else {
        return (String::new(), "");
    };
    let value = display(
        &state
            .effective(path)
            .unwrap_or_else(|| item.default.clone()),
    );
    if value.trim().is_empty() {
        return (String::new(), "");
    }
    let errors = keys::validate(&value, kind, item.accepts_range);
    if let Some(first) = errors.first() {
        return (first.clone(), "err");
    }
    let r = keys::risk(&value, kind);
    (r.reason.to_string(), r.level)
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
    app.set_entries(entry_cards(state));
    app.set_section_is_list(section_is_list(state) && state.filter.trim().is_empty());
    app.set_section_title(section_title(state).into());
    app.set_selected(state.selected as i32);
    app.set_dirty_count((state.dirty_count() + state.removed_entries.len()) as i32);
    app.set_allowed_tokens(allowed_tokens(state));
    app.set_filter(state.filter.clone().into());
    refresh_picker(app, state);
    refresh_capture(app, state);
    refresh_problems(app, state);
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
        problems_open: false,
        diff_open: false,
        removed_entries: Default::default(),
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
            let is_size = state
                .borrow()
                .item(&path)
                .is_some_and(|i| i.size);
            let next = if is_size {
                match size_to_toml(&value) {
                    Ok(v) => v,
                    // Keep the previous value rather than writing something
                    // herdr would refuse.
                    Err(_) => return,
                }
            } else {
                let ty = types.get(&path).cloned().unwrap_or_else(|| "string".into());
                to_toml(&value, &ty)
            };
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
            s.problems_open = false;
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
        app.on_capture_pressed(move |key, _slint_ctrl, _slint_shift, _slint_alt, _slint_meta| {
            // Slint's winit backend swaps Control and Command on Apple
            // platforms, so its modifiers name the wrong key for a binding
            // that has to reach a terminal. winit's are taken instead.
            let m = physical::modifiers();
            let (ctrl, shift, alt, meta) = (m.ctrl, m.shift, m.alt, m.meta);
            // Raw record of what Slint hands us, so real hardware presses can
            // be compared against what the browser build sees.
            eprintln!(
                "[key] slint={key:?} physical={:?} raw={} codepoints=[{}] ctrl={ctrl} shift={shift} alt={alt} meta={meta} (slint said ctrl={_slint_ctrl} meta={_slint_meta})",
                physical::last(),
                physical::last_raw(),
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
            let mut s = state.borrow_mut();
            s.edits.clear();
            s.removed_entries.clear();
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
            app.set_result("取り消しました".into());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_add_entry(move || {
            let section = {
                let s = state.borrow();
                s.schema
                    .sections
                    .get(s.selected)
                    .map(|x| x.name.clone())
            };
            if let Some(section) = section {
                state.borrow_mut().add_entry(&section);
            }
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_remove_entry(move |index| {
            let section = {
                let s = state.borrow();
                s.schema
                    .sections
                    .get(s.selected)
                    .map(|x| x.name.clone())
            };
            if let Some(section) = section {
                state.borrow_mut().remove_entry(&section, index.max(0) as usize);
            }
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_open_problems(move || {
            state.borrow_mut().problems_open = true;
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_close_problems(move || {
            state.borrow_mut().problems_open = false;
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_problem_goto(move |path| {
            let mut s = state.borrow_mut();
            // Jumping to a setting is also how you leave the panel.
            s.problems_open = false;
            s.filter = path.to_string();
            drop(s);
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
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
            if edits.is_empty() || !state.borrow().has_pending() {
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
            s.removed_entries.clear();
            s.diff_open = false;
            drop(s);
            refresh(&app, &state.borrow());
            app.set_result(text.into());
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_preview(move || {
            let app = weak.unwrap();
            let s = state.borrow();
            let edits = s.edits();
            if edits.is_empty() {
                return;
            }
            let raw = config::config_path()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default();
            drop(s);

            match config::apply_edits(&raw, &edits) {
                Ok((after, changes)) => {
                    // Ask herdr about the exact bytes a save would write.
                    let report = check::check_toml(&after);
                    let rows: Vec<DiffRow> = changes
                        .iter()
                        .filter(|c| c.action != "noop")
                        .map(|c| DiffRow {
                            action: c.action.into(),
                            path: c.path.clone().into(),
                            from: c
                                .from
                                .clone()
                                .unwrap_or_else(|| "(既定)".into())
                                .into(),
                            to: c.to.clone().unwrap_or_else(|| "(既定に戻す)".into()).into(),
                        })
                        .collect();
                    app.set_diff_rows(ModelRc::new(VecModel::from(rows)));
                    app.set_diff_check(
                        if report.ok { String::new() } else { report.raw.clone() }.into(),
                    );
                    app.set_diff_check_ok(report.ok);
                    app.set_diff_fatal(report.fatal());
                    app.set_diff_after(after.into());
                    state.borrow_mut().diff_open = true;
                    refresh(&app, &state.borrow());
                }
                Err(e) => app.set_result(e.into()),
            }
        });
    }
    {
        let state = state.clone();
        let weak = app.as_weak();
        app.on_close_diff(move || {
            state.borrow_mut().diff_open = false;
            let app = weak.unwrap();
            refresh(&app, &state.borrow());
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

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn a_percentage_is_quoted_and_a_cell_count_is_not() {
        // herdr: `string sizes must be percentages like 80%; use a number for cells`
        assert_eq!(size_to_toml("80%").unwrap().as_deref(), Some("\"80%\""));
        assert_eq!(size_to_toml("120").unwrap().as_deref(), Some("120"));
        assert_eq!(size_to_toml(" 40 ").unwrap().as_deref(), Some("40"));
    }

    #[test]
    fn percentages_outside_one_to_a_hundred_are_refused() {
        assert!(size_to_toml("0%").is_err());
        assert!(size_to_toml("200%").is_err());
        assert_eq!(size_to_toml("1%").unwrap().as_deref(), Some("\"1%\""));
        assert_eq!(size_to_toml("100%").unwrap().as_deref(), Some("\"100%\""));
    }

    #[test]
    fn an_empty_field_still_means_inherit() {
        assert_eq!(size_to_toml("").unwrap(), None);
        assert_eq!(size_to_toml("   ").unwrap(), None);
    }

    #[test]
    fn anything_that_is_neither_is_refused_rather_than_guessed_at() {
        for bad in ["big", "80 %", "80px", "-10"] {
            assert!(size_to_toml(bad).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
mod path_helper_tests {
    use super::*;

    #[test]
    fn an_index_is_stripped_for_schema_lookups() {
        assert_eq!(strip_index("keys.command[0].key"), "keys.command.key");
        assert_eq!(strip_index("theme.name"), "theme.name");
        assert_eq!(strip_index("keys.command[12].width"), "keys.command.width");
    }

    #[test]
    fn the_trailing_index_is_read_back() {
        assert_eq!(entry_index_of("keys.command[2]"), Some(2));
        assert_eq!(entry_index_of("keys.command[0]"), Some(0));
        assert_eq!(entry_index_of("keys.command[0].key"), None, "not an entry path");
        assert_eq!(entry_index_of("theme.name"), None);
    }
}
