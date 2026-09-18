//! Binary-driven schema: derive the settings form from `herdr --default-config`.
//!
//! The default config is read line-by-line, NOT through a TOML parser: almost
//! every setting ships commented out, and a TOML parser would discard exactly
//! the lines we need (both the `# key = value` defaults and the prose that
//! documents them). Commented-out section headers such as `# [theme.custom]`
//! must be tracked too, or their keys get attributed to the wrong table.

use regex::Regex;
use serde::Serialize;
use toml_edit::Value;

use crate::overlay;

#[derive(Serialize, Clone)]
pub struct Item {
    pub line: usize,
    /// Dotted path used as the identity of a setting, e.g. `ui.toast.delivery`.
    pub path: String,
    pub section: String,
    pub key: String,
    /// `bool` | `integer` | `float` | `string` | `array` | `table` | `datetime`
    pub ty: String,
    /// Default value, verbatim TOML source text.
    pub default: String,
    /// Doc-comment lines immediately above the setting.
    pub doc: Vec<String>,
    /// Trailing `# ...` comment on the same line.
    pub trailing: String,
    /// Quoted literals harvested from the doc block; likely enum members.
    pub enum_candidates: Vec<String>,
    /// True when the default is `""` and the docs call it optional/unset.
    pub optional: bool,
    /// True when the default is NOT empty but the docs say an empty string
    /// turns the feature off. Such a setting needs a way to write `""`
    /// explicitly, which "clear the field to inherit the default" cannot do.
    pub empty_disables: bool,
    pub is_key_binding: bool,
    /// Which set of syntax rules applies to this binding, if it is one.
    /// `prefix` the prefix chord itself, `action` a normal prefix-mode or
    /// direct binding, `navigate` a plain key that only applies while
    /// navigate mode is open, `indexed` a modifier-only value driving 1..9,
    /// `command` the key of a `[[keys.command]]` entry.
    pub binding_kind: Option<&'static str>,
    /// Accepts the `1..9` range form (e.g. `switch_tab = "prefix+1..9"`).
    pub accepts_range: bool,
    /// The value is a color, so the form offers a picker. herdr does not
    /// validate colors, so this is the only place a bad one gets caught.
    pub color: bool,
    /// The value is a popup dimension: `"80%"` as a string or a cell count as
    /// a bare integer.
    pub size: bool,
    /// Supplied by the hand-written overlay rather than by
    /// `herdr --default-config`, which documents only some table members.
    pub from_overlay: bool,
}

#[derive(Serialize, Clone)]
pub struct Section {
    pub name: String,
    pub line: usize,
    /// The header itself was commented out in the default config.
    pub commented: bool,
    pub array_of_tables: bool,
    pub doc: Vec<String>,
    pub items: Vec<Item>,
    /// Prose lines shaped like `key = value` but not valid TOML. These are
    /// documentation, not settings -- most describe the allowed values of a
    /// nearby setting, so we keep them as hints instead of dropping them.
    pub hints: Vec<Hint>,
}

#[derive(Serialize, Clone)]
pub struct Hint {
    pub line: usize,
    pub name: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct Schema {
    pub sections: Vec<Section>,
    pub item_count: usize,
    pub hint_count: usize,
}

/// An item the overlay contributes: no documented default, so an empty field
/// means "not overridden".
fn overlay_item(section: &str, key: &str) -> Item {
    Item {
        line: 0,
        path: format!("{section}.{key}"),
        section: section.to_string(),
        key: key.to_string(),
        ty: "string".to_string(),
        default: String::new(),
        doc: Vec::new(),
        trailing: String::new(),
        enum_candidates: Vec::new(),
        optional: false,
        empty_disables: false,
        is_key_binding: false,
        binding_kind: None,
        accepts_range: false,
        color: true,
        size: false,
        from_overlay: true,
    }
}

/// Fill in the table members `--default-config` only demonstrates.
///
/// `[theme.custom]` shows seven of nineteen color tokens; the rest are real
/// settings herdr accepts (each one probed in `overlay`'s tests) that the form
/// could otherwise never reach.
fn section_index(schema: &mut Schema, name: &str) -> usize {
    match schema.sections.iter().position(|s| s.name == name) {
        Some(i) => i,
        None => {
            schema.sections.push(Section {
                name: name.to_string(),
                line: 0,
                commented: true,
                array_of_tables: false,
                doc: Vec::new(),
                items: Vec::new(),
                hints: Vec::new(),
            });
            schema.sections.len() - 1
        }
    }
}

pub fn augment(schema: &mut Schema) {
    for table in overlay::THEME_TABLES {
        let idx = section_index(schema, table);
        let section = &mut schema.sections[idx];
        for token in overlay::THEME_TOKENS {
            if !section.items.iter().any(|i| i.key == *token) {
                section.items.push(overlay_item(table, token));
            }
        }
        // Present them in the overlay's order so surfaces, text and palette
        // stay grouped regardless of which ones happened to be documented.
        section.items.sort_by_key(|i| {
            overlay::THEME_TOKENS
                .iter()
                .position(|t| *t == i.key)
                .unwrap_or(usize::MAX)
        });
    }

    // Settings herdr accepts but never documents.
    for extra in overlay::EXTRA_SETTINGS {
        let idx = section_index(schema, extra.section);
        let section = &mut schema.sections[idx];
        if section.items.iter().any(|i| i.key == extra.key) {
            continue;
        }
        section.items.push(Item {
            line: 0,
            path: format!("{}.{}", extra.section, extra.key),
            section: extra.section.to_string(),
            key: extra.key.to_string(),
            ty: extra.ty.to_string(),
            default: String::new(),
            doc: vec![extra.doc.to_string()],
            trailing: String::new(),
            enum_candidates: extra.enum_values.iter().map(|s| s.to_string()).collect(),
            optional: false,
            empty_disables: false,
            is_key_binding: extra.binding_kind.is_some(),
            binding_kind: extra.binding_kind,
            accepts_range: false,
            color: overlay::is_color(extra.section, extra.key),
            size: overlay::is_size(extra.section, extra.key),
            from_overlay: true,
        });
    }

    // Replace prose-scraped enum candidates with the members herdr states.
    for o in overlay::ENUM_OVERRIDES {
        if let Some(item) = schema
            .sections
            .iter_mut()
            .find(|s| s.name == o.section)
            .and_then(|s| s.items.iter_mut().find(|i| i.key == o.key))
        {
            item.enum_candidates = o.members.iter().map(|m| m.to_string()).collect();
        }
    }

    schema.item_count = schema.sections.iter().map(|s| s.items.len()).sum();
}

/// The schema the UI consumes: parsed from herdr, then completed by the overlay.
pub fn build(default_config: &str) -> Schema {
    let mut schema = parse(default_config);
    augment(&mut schema);
    schema
}

fn infer_type(raw: &str) -> Option<&'static str> {
    let parsed: Value = raw.parse().ok()?;
    Some(match parsed {
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "bool",
        Value::Datetime(_) => "datetime",
        Value::Array(_) => "array",
        Value::InlineTable(_) => "table",
    })
}

/// Enum members documented as bare assignments, e.g. the `[ui.toast]` block's
/// `off = disable pop-up notifications`. Those lines are prose (we reject them
/// as settings) but their left-hand sides are exactly the allowed values.
fn documented_alternatives(doc: &[String]) -> Vec<String> {
    let re = Regex::new(r"^([A-Za-z_][A-Za-z0-9_\-]*)\s*=\s*\S").unwrap();
    let mut out: Vec<String> = Vec::new();
    for line in doc {
        if let Some(c) = re.captures(line) {
            let v = c[1].to_string();
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// herdr applies different syntax rules per binding family, and the rules are
/// stated in prose rather than machine-readable form, so they are keyed off
/// the table and key name here.
fn binding_kind(section: &str, key: &str, ty: &str) -> Option<&'static str> {
    if ty != "string" {
        return None;
    }
    match section {
        "keys" => Some(if key == "prefix" {
            "prefix"
        } else if key.starts_with("navigate_") {
            "navigate"
        } else {
            "action"
        }),
        "keys.indexed" => Some("indexed"),
        // `[[keys.command]]` also holds type/command/width/height, which are
        // not bindings.
        "keys.command" => (key == "key").then_some("command"),
        _ => None,
    }
}

fn quoted_literals(doc: &[String]) -> Vec<String> {
    let re = Regex::new(r#""([^"\\]*)""#).unwrap();
    let mut out: Vec<String> = Vec::new();
    for line in doc {
        for c in re.captures_iter(line) {
            let v = c[1].to_string();
            if !v.is_empty() && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

pub fn parse(text: &str) -> Schema {
    let re_section = Regex::new(r"^(\[\[?)([A-Za-z0-9_.\-]+)(\]\]?)$").unwrap();
    let re_kv = Regex::new(r"^([A-Za-z_][A-Za-z0-9_\-]*)\s*=\s*(.*?)(?:\s+#\s*(.*))?$").unwrap();

    let mut sections: Vec<Section> = Vec::new();
    let mut current = String::new(); // "" == root table
                                     // A commented-out header such as `# [theme.custom]` only governs the
                                     // comment block it heads. `[ui]`'s own `accent` sits after the
                                     // `# [ui.sidebar.spaces]` block, separated by a blank line, and belongs to
                                     // `[ui]` -- herdr rejects `ui.sidebar.spaces.accent`. So a blank line ends
                                     // a commented header's scope and restores the last real one.
    let mut real_section = String::new();
    let mut doc: Vec<String> = Vec::new();

    // Root pseudo-section so top-level keys (e.g. `onboarding`) have a home.
    sections.push(Section {
        name: String::new(),
        line: 1,
        commented: false,
        array_of_tables: false,
        doc: Vec::new(),
        items: Vec::new(),
        hints: Vec::new(),
    });

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            doc.clear();
            current = real_section.clone();
            continue;
        }

        let commented = trimmed.starts_with('#');
        let body = if commented {
            trimmed.trim_start_matches('#').trim()
        } else {
            trimmed
        };
        if body.is_empty() {
            continue;
        }

        if let Some(c) = re_section.captures(body) {
            let open = &c[1];
            let name = c[2].to_string();
            let array_of_tables = open == "[[";
            current = name.clone();
            if !commented {
                real_section = name.clone();
            }
            if !sections.iter().any(|s| s.name == name) {
                sections.push(Section {
                    name,
                    line: line_no,
                    commented,
                    array_of_tables,
                    doc: std::mem::take(&mut doc),
                    items: Vec::new(),
                    hints: Vec::new(),
                });
            }
            doc.clear();
            continue;
        }

        if let Some(c) = re_kv.captures(body) {
            let key = c[1].to_string();
            let value = c
                .get(2)
                .map(|m| m.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let trailing = c.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();

            let sec_idx = sections.iter().position(|s| s.name == current).unwrap_or(0);

            match infer_type(&value) {
                Some(ty) => {
                    let path = if current.is_empty() {
                        key.clone()
                    } else {
                        format!("{current}.{key}")
                    };
                    let doc_block = std::mem::take(&mut doc);
                    let mut enum_candidates = if ty == "string" {
                        quoted_literals(&doc_block)
                    } else {
                        Vec::new()
                    };
                    if ty == "string" {
                        for alt in documented_alternatives(&doc_block) {
                            if !enum_candidates.contains(&alt) {
                                enum_candidates.push(alt);
                            }
                        }
                    }
                    let hay = format!("{} {}", doc_block.join(" "), trailing).to_lowercase();
                    let optional = value == "\"\""
                        && (hay.contains("optional")
                            || hay.contains("unset")
                            || hay.contains("disable"));
                    let empty_disables = ty == "string"
                        && value != "\"\""
                        && (hay.contains("empty") || hay.contains("set to \"\""));
                    let is_key_binding = current == "keys" || current.starts_with("keys.");
                    let color = overlay::is_color(&current, &key);
                    let size = overlay::is_size(&current, &key);
                    let binding_kind = binding_kind(&current, &key, ty);
                    let accepts_range = binding_kind.is_some()
                        && (value.contains("1..9")
                            || hay.contains("indexed binding")
                            || binding_kind == Some("indexed"));

                    sections[sec_idx].items.push(Item {
                        line: line_no,
                        path,
                        section: current.clone(),
                        key,
                        ty: ty.to_string(),
                        default: value,
                        doc: doc_block,
                        trailing,
                        enum_candidates,
                        optional,
                        empty_disables,
                        is_key_binding,
                        binding_kind,
                        accepts_range,
                        color,
                        size,
                        from_overlay: false,
                    });
                }
                None => {
                    // Prose that merely looks like an assignment.
                    let description = if trailing.is_empty() {
                        value
                    } else {
                        format!("{value} # {trailing}")
                    };
                    sections[sec_idx].hints.push(Hint {
                        line: line_no,
                        name: key,
                        description,
                    });
                    doc.push(body.to_string());
                }
            }
            continue;
        }

        if commented {
            doc.push(body.to_string());
        }
    }

    sections.retain(|s| !(s.items.is_empty() && s.hints.is_empty()));
    let item_count = sections.iter().map(|s| s.items.len()).sum();
    let hint_count = sections.iter().map(|s| s.hints.len()).sum();
    Schema {
        sections,
        item_count,
        hint_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A committed snapshot of `herdr --default-config`, so the suite runs on
    /// machines (and CI runners) without herdr installed. The counts below pin
    /// the behaviours we care about; `fixture_matches_installed_herdr` catches
    /// drift whenever herdr *is* present.
    const FIXTURE: &str = include_str!("../fixtures/default-config.toml");

    use crate::check::Kind;

    fn real() -> Schema {
        parse(FIXTURE)
    }

    #[test]
    fn the_overlay_completes_the_theme_tables() {
        let s = build(FIXTURE);
        for table in crate::overlay::THEME_TABLES {
            let sec = s
                .sections
                .iter()
                .find(|x| x.name == *table)
                .unwrap_or_else(|| panic!("missing {table}"));
            let keys: Vec<&str> = sec.items.iter().map(|i| i.key.as_str()).collect();
            assert_eq!(
                keys,
                crate::overlay::THEME_TOKENS.to_vec(),
                "{table} must expose every token, in the overlay's order"
            );
            assert!(sec.items.iter().all(|i| i.color), "all tokens are colors");
        }

        // `--default-config` documents 7 of 19 in [theme.custom] and 2 of 19
        // in each of light/dark, so 46 tokens were previously unreachable.
        let documented = parse(FIXTURE);
        assert_eq!(documented.item_count, 140);
        assert_eq!(
            s.item_count, 193,
            "140 documented + 46 theme tokens + 7 undocumented"
        );

        let custom = s
            .sections
            .iter()
            .find(|x| x.name == "theme.custom")
            .unwrap();
        let documented_token = custom.items.iter().find(|i| i.key == "accent").unwrap();
        assert!(!documented_token.from_overlay);
        assert_eq!(documented_token.default, "\"#f5c2e7\"");

        let added = custom.items.iter().find(|i| i.key == "teal").unwrap();
        assert!(added.from_overlay);
        assert_eq!(
            added.default, "",
            "there is no default to inherit, only the base theme"
        );
    }

    #[test]
    fn the_overlay_adds_the_settings_herdr_does_not_document() {
        let s = build(FIXTURE);
        let keys = s.sections.iter().find(|x| x.name == "keys").unwrap();

        // Documented [keys] holds 54; the overlay adds five more.
        assert_eq!(keys.items.len(), 59);
        for key in [
            "swap_pane_left",
            "swap_pane_down",
            "swap_pane_up",
            "swap_pane_right",
            "copy_mode",
        ] {
            let item = keys
                .items
                .iter()
                .find(|i| i.key == key)
                .unwrap_or_else(|| panic!("missing {key}"));
            assert!(item.from_overlay);
            // They must reach the key capture and conflict machinery.
            assert_eq!(item.binding_kind, Some("action"));
            assert!(item.is_key_binding);
            assert!(!item.accepts_range);
        }

        // `fullscreen` is a legacy alias of `zoom` and must stay out.
        assert!(!keys.items.iter().any(|i| i.key == "fullscreen"));

        // `[[keys.command]]` gains an undocumented label field, and its `type`
        // enum gains the member `--default-config` omits.
        let cmd = s
            .sections
            .iter()
            .find(|x| x.name == "keys.command")
            .unwrap();
        assert!(cmd.array_of_tables);
        let desc = cmd.items.iter().find(|i| i.key == "description").unwrap();
        assert!(desc.from_overlay);
        let ty = cmd.items.iter().find(|i| i.key == "type").unwrap();
        assert_eq!(
            ty.enum_candidates,
            ["shell", "pane", "popup", "plugin_action"]
        );
        for k in ["width", "height"] {
            assert!(cmd.items.iter().find(|i| i.key == k).unwrap().size, "{k}");
        }

        let ui = s.sections.iter().find(|x| x.name == "ui").unwrap();
        let scope = ui
            .items
            .iter()
            .find(|i| i.key == "agent_panel_scope")
            .unwrap();
        assert_eq!(scope.enum_candidates, ["current", "all"]);
        assert!(scope.from_overlay);

        // `advanced.scrollback_lines` is an alias of `scrollback_limit_bytes`,
        // not a separate setting: herdr reports `duplicate field` when both
        // are present, so offering it would produce a config herdr refuses.
        let adv = s.sections.iter().find(|x| x.name == "advanced").unwrap();
        assert!(!adv.items.iter().any(|i| i.key == "scrollback_lines"));
        assert!(adv
            .items
            .iter()
            .any(|i| i.key == "scrollback_limit_bytes" && !i.from_overlay));
    }

    #[test]
    fn augmenting_twice_changes_nothing() {
        let mut once = build(FIXTURE);
        let before = once.item_count;
        augment(&mut once);
        assert_eq!(once.item_count, before, "augment must be idempotent");
    }

    #[test]
    fn ui_accent_is_a_color_but_its_neighbours_are_not() {
        let s = build(FIXTURE);
        let ui = s.sections.iter().find(|x| x.name == "ui").unwrap();
        assert!(ui.items.iter().find(|i| i.key == "accent").unwrap().color);
        assert!(
            !ui.items
                .iter()
                .find(|i| i.key == "sidebar_width")
                .unwrap()
                .color
        );
    }

    /// Every setting must live at a path herdr actually recognises.
    ///
    /// This is what caught `accent` being attributed to `[ui.sidebar.spaces]`
    /// instead of `[ui]`: writing a config that mentions all of them at once
    /// and letting herdr name the ones it does not know. Skipped when herdr is
    /// unavailable.
    #[test]
    fn every_setting_sits_where_herdr_expects_it() {
        if crate::check::check_toml("").unavailable.is_some() {
            eprintln!("herdr not installed; skipping");
            return;
        }
        let schema = build(FIXTURE);
        let mut lines = Vec::new();
        let mut count = 0;
        for sec in &schema.sections {
            // An array-of-tables cannot be written as a plain table, so it is
            // covered by its own test instead.
            if sec.array_of_tables || sec.items.is_empty() {
                continue;
            }
            if !sec.name.is_empty() {
                lines.push(format!("[{}]", sec.name));
            }
            for item in &sec.items {
                // Overlay items have no documented default, so pick something
                // of the right shape: an enum member, a number, or a color.
                let value = if !item.default.is_empty() {
                    item.default.clone()
                } else if let Some(first) = item.enum_candidates.first() {
                    format!("\"{first}\"")
                } else {
                    match item.ty.as_str() {
                        "integer" => "0".to_string(),
                        "float" => "0.0".to_string(),
                        "bool" => "false".to_string(),
                        _ => "\"#112233\"".to_string(),
                    }
                };
                lines.push(format!("{} = {value}", item.key));
                count += 1;
            }
            lines.push(String::new());
        }

        let report = crate::check::check_toml(&lines.join("\n"));
        let misplaced: Vec<&str> = report
            .diagnostics
            .iter()
            .filter(|d| matches!(d.kind, Kind::UnknownKey | Kind::UnknownSection))
            .map(|d| d.message.as_str())
            .collect();
        assert!(count > 150, "the sweep must be comprehensive, got {count}");
        assert!(
            misplaced.is_empty(),
            "herdr does not recognise these paths: {misplaced:#?}"
        );
        assert!(
            !report.discards_config,
            "the sweep must parse: {}",
            report.raw
        );
    }

    #[test]
    fn fixture_matches_installed_herdr() {
        let Ok(live) = crate::herdr::default_config() else {
            eprintln!("herdr not installed; skipping drift check");
            return;
        };
        assert_eq!(
            live.replace("\r\n", "\n"),
            FIXTURE.replace("\r\n", "\n"),
            "fixtures/default-config.toml is stale. Refresh it with:\n  \
             herdr --default-config > src-tauri/fixtures/default-config.toml\n\
             then update the counts in these tests if the schema really changed."
        );
    }

    #[test]
    fn extracts_every_setting() {
        let s = real();
        assert_eq!(s.item_count, 140, "settable keys");
        // 24 bracketed headers plus the root pseudo-section holding `onboarding`.
        assert_eq!(s.sections.len(), 25, "sections");
        let root = s.sections.iter().find(|x| x.name.is_empty()).unwrap();
        assert_eq!(
            root.items
                .iter()
                .map(|i| i.path.as_str())
                .collect::<Vec<_>>(),
            ["onboarding"]
        );
    }

    #[test]
    fn rejects_prose_that_looks_like_an_assignment() {
        let s = real();
        assert_eq!(s.hint_count, 7, "prose lines shaped like key = value");

        // `# off = disable pop-up notifications` documents the allowed values
        // of ui.toast.delivery. It must never become a setting called `off`.
        let toast = s.sections.iter().find(|x| x.name == "ui.toast").unwrap();
        let hints: Vec<&str> = toast.hints.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(hints, ["off", "herdr", "terminal", "system"]);
        assert!(!toast.items.iter().any(|i| i.key == "off"));
        assert!(toast.items.iter().any(|i| i.key == "delivery"));
    }

    #[test]
    fn tracks_commented_out_section_headers() {
        let s = real();
        let commented: Vec<&str> = s
            .sections
            .iter()
            .filter(|x| x.commented)
            .map(|x| x.name.as_str())
            .collect();
        assert!(commented.contains(&"theme.custom"));
        assert!(commented.contains(&"ui.sidebar.agents"));
        assert_eq!(commented.len(), 10);

        // `rows` lives under [ui.sidebar.agents], not under [ui].
        let ui = s.sections.iter().find(|x| x.name == "ui").unwrap();
        assert!(!ui.items.iter().any(|i| i.key == "rows"));
        let agents = s
            .sections
            .iter()
            .find(|x| x.name == "ui.sidebar.agents")
            .unwrap();
        assert!(agents
            .items
            .iter()
            .any(|i| i.path == "ui.sidebar.agents.rows"));
    }

    #[test]
    fn keys_section_dominates_and_is_flagged() {
        let s = real();
        let keys = s.sections.iter().find(|x| x.name == "keys").unwrap();
        assert_eq!(keys.items.len(), 54);
        assert!(keys.items.iter().all(|i| i.is_key_binding));
        let prefix = keys.items.iter().find(|i| i.key == "prefix").unwrap();
        assert_eq!(prefix.default, "\"ctrl+b\"");
        // Bindings that ship unset are offered as "optional", not as "".
        assert!(
            keys.items
                .iter()
                .find(|i| i.key == "open_worktree")
                .unwrap()
                .optional
        );
    }

    #[test]
    fn classifies_binding_families() {
        let s = real();
        let get = |path: &str| {
            s.sections
                .iter()
                .flat_map(|sec| sec.items.iter())
                .find(|i| i.path == path)
                .unwrap_or_else(|| panic!("missing {path}"))
        };

        assert_eq!(get("keys.prefix").binding_kind, Some("prefix"));
        assert_eq!(get("keys.split_vertical").binding_kind, Some("action"));
        assert_eq!(
            get("keys.navigate_pane_left").binding_kind,
            Some("navigate")
        );
        assert_eq!(get("keys.indexed.tabs").binding_kind, Some("indexed"));
        assert_eq!(get("keys.command.key").binding_kind, Some("command"));

        // The other `[[keys.command]]` fields are not bindings.
        for k in ["type", "command", "width", "height"] {
            assert_eq!(get(&format!("keys.command.{k}")).binding_kind, None, "{k}");
        }
        // Nothing outside the keys tables is a binding.
        assert_eq!(get("theme.name").binding_kind, None);

        let navigate: Vec<&str> = s
            .sections
            .iter()
            .flat_map(|sec| sec.items.iter())
            .filter(|i| i.binding_kind == Some("navigate"))
            .map(|i| i.key.as_str())
            .collect();
        assert_eq!(navigate.len(), 6, "navigate-mode bindings: {navigate:?}");
    }

    #[test]
    fn flags_bindings_that_accept_the_1_to_9_range() {
        let s = real();
        let ranged: Vec<&str> = s
            .sections
            .iter()
            .flat_map(|sec| sec.items.iter())
            .filter(|i| i.accepts_range)
            .map(|i| i.path.as_str())
            .collect();
        assert_eq!(
            ranged,
            [
                // Declaration order in the default config.
                "keys.focus_agent",
                "keys.switch_tab",
                "keys.switch_workspace",
                "keys.indexed.tabs",
                "keys.indexed.workspaces",
                "keys.indexed.agents",
            ]
        );
    }

    #[test]
    fn flags_settings_where_an_empty_string_is_meaningful() {
        let s = real();
        let flagged: Vec<&str> = s
            .sections
            .iter()
            .flat_map(|sec| sec.items.iter())
            .filter(|i| i.empty_disables)
            .map(|i| i.path.as_str())
            .collect();
        // These two default to a real value but document "" as an off switch,
        // so clearing the field cannot express them -- the UI needs an
        // explicit "disable" action.
        assert_eq!(flagged, ["keys.remote_image_paste", "ui.window_title"]);

        // Settings that already default to "" need no such action.
        let term = s.sections.iter().find(|x| x.name == "terminal").unwrap();
        let shell = term
            .items
            .iter()
            .find(|i| i.key == "default_shell")
            .unwrap();
        assert!(shell.optional || shell.default == "\"\"");
        assert!(!shell.empty_disables);
    }

    #[test]
    fn harvests_enum_candidates_from_doc_prose() {
        let s = real();
        let term = s.sections.iter().find(|x| x.name == "terminal").unwrap();
        let mode = term.items.iter().find(|i| i.key == "shell_mode").unwrap();
        for want in ["auto", "login", "non_login"] {
            assert!(
                mode.enum_candidates.contains(&want.to_string()),
                "missing {want}"
            );
        }
    }

    #[test]
    fn rejected_prose_becomes_enum_candidates() {
        let s = real();
        let toast = s.sections.iter().find(|x| x.name == "ui.toast").unwrap();
        let delivery = toast.items.iter().find(|i| i.key == "delivery").unwrap();
        for want in ["off", "herdr", "terminal", "system"] {
            assert!(
                delivery.enum_candidates.contains(&want.to_string()),
                "ui.toast.delivery missing candidate {want}: {:?}",
                delivery.enum_candidates
            );
        }
    }

    #[test]
    fn array_of_tables_is_marked() {
        let s = real();
        let cmd = s
            .sections
            .iter()
            .find(|x| x.name == "keys.command")
            .unwrap();
        assert!(cmd.array_of_tables);
        assert!(cmd.commented);
        assert_eq!(cmd.items.len(), 5);
    }
}
