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

/// The rows editor's own logic, which has no counterpart in the Tauri build:
/// there it lives in TypeScript as `src/rows.ts`.
mod rows;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

slint::include_modules!();

/// Everything the window needs, kept on the Rust side.
struct State {
    schema: schema::Schema,
    /// Values on disk, as verbatim TOML text.
    saved: BTreeMap<String, String>,
    /// Touched settings; `None` means "back to the default".
    edits: BTreeMap<String, Option<String>>,
    selected: usize,
}

impl State {
    fn effective(&self, path: &str) -> Option<String> {
        match self.edits.get(path) {
            Some(v) => v.clone(),
            None => self.saved.get(path).cloned(),
        }
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

fn item_rows(state: &State) -> ModelRc<ItemRow> {
    let Some(section) = state.schema.sections.get(state.selected) else {
        return ModelRc::new(VecModel::from(Vec::<ItemRow>::new()));
    };
    let rows: Vec<ItemRow> = section
        .items
        .iter()
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
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn section_title(state: &State) -> String {
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
    }));

    let app = App::new()?;
    app.set_meta(meta.into());
    app.set_result("書き込みは行いません（試作のため読み取り専用）".into());
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
        let state = state.clone();
        let weak = app.as_weak();
        app.on_select_section(move |index| {
            state.borrow_mut().selected = index.max(0) as usize;
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
        let weak = app.as_weak();
        app.on_swatch_clicked(move |path| {
            // A real picker is the next thing to build; this proves the
            // swatch renders and is clickable.
            weak.unwrap()
                .set_result(format!("カラーピッカー未実装: {path}").into());
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
        app.on_preview(move || {
            let s = state.borrow();
            let edits: Vec<config::Edit> = s
                .edits
                .iter()
                .filter(|(p, _)| s.is_dirty(p))
                .map(|(path, value)| config::Edit {
                    path: path.clone(),
                    value: value.clone(),
                    op: config::Op::Set,
                })
                .collect();
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

    app.run()
}
