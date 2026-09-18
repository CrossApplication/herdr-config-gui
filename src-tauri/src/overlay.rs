//! The hand-written layer on top of the binary-derived schema.
//!
//! `herdr --default-config` is documentation, not a complete schema: it
//! demonstrates a few members of some tables rather than listing them. Those
//! gaps are filled here, and a test probes the installed herdr for every name
//! so this file cannot quietly drift from reality.
//!
//! Keeping this layer small is the whole point. Anything that can be read out
//! of herdr belongs in `schema.rs` or `check.rs` instead.

/// Every color token `[theme.custom]` accepts, in a display order that groups
/// surfaces, then text, then the palette. `--default-config` shows only seven
/// of them, so the rest would otherwise be unreachable from the form.
///
/// `[theme.custom.light]` and `[theme.custom.dark]` take the same set.
pub const THEME_TOKENS: &[&str] = &[
    "panel_bg",
    "sidebar_bg",
    "active_row_bg",
    "selection_bg",
    "surface0",
    "surface1",
    "surface_dim",
    "overlay0",
    "overlay1",
    "text",
    "subtext0",
    "accent",
    "mauve",
    "blue",
    "teal",
    "green",
    "yellow",
    "peach",
    "red",
];

/// Tables that take the full token set.
pub const THEME_TABLES: &[&str] = &["theme.custom", "theme.custom.light", "theme.custom.dark"];

/// A setting herdr accepts that `--default-config` never mentions.
///
/// Each one was found in the binary's config struct and confirmed with
/// `herdr config check`, which also reported the type and, for enums, the
/// members. There is no documented default, so an empty field means "leave it
/// to herdr" exactly as it does for a color token.
pub struct Extra {
    pub section: &'static str,
    pub key: &'static str,
    /// `string` | `integer` | `bool`, matching schema::Item::ty.
    pub ty: &'static str,
    /// Our own description: herdr documents none of these.
    pub doc: &'static str,
    /// Members herdr listed when it rejected a bad value.
    pub enum_values: &'static [&'static str],
    /// Set for keybindings so they get capture, validation and conflicts.
    pub binding_kind: Option<&'static str>,
}

/// Aliases are deliberately absent. `keys.fullscreen` is described in
/// `--default-config` as a legacy alias of `zoom`, and `advanced.
/// scrollback_lines` turned out to be one for `scrollback_limit_bytes`:
/// setting a name and its alias together makes herdr report `duplicate
/// field`, which is how an alias can be told apart from a real setting that
/// merely happens to be undocumented. Offering both names for one value would
/// let the form write a config herdr refuses outright.
///
/// `schema`'s attribution sweep writes every setting at once, so it fails if
/// an alias ever slips into this list.
pub const EXTRA_SETTINGS: &[Extra] = &[
    Extra {
        section: "keys",
        key: "swap_pane_left",
        ty: "string",
        doc: "Swap the focused pane with the pane to its left.",
        enum_values: &[],
        binding_kind: Some("action"),
    },
    Extra {
        section: "keys",
        key: "swap_pane_down",
        ty: "string",
        doc: "Swap the focused pane with the pane below it.",
        enum_values: &[],
        binding_kind: Some("action"),
    },
    Extra {
        section: "keys",
        key: "swap_pane_up",
        ty: "string",
        doc: "Swap the focused pane with the pane above it.",
        enum_values: &[],
        binding_kind: Some("action"),
    },
    Extra {
        section: "keys",
        key: "swap_pane_right",
        ty: "string",
        doc: "Swap the focused pane with the pane to its right.",
        enum_values: &[],
        binding_kind: Some("action"),
    },
    Extra {
        section: "keys",
        key: "copy_mode",
        ty: "string",
        doc: "Enter copy mode in the focused pane.",
        enum_values: &[],
        binding_kind: Some("action"),
    },
    Extra {
        section: "ui",
        key: "agent_panel_scope",
        ty: "string",
        doc: "Which agents the agent panel lists.",
        enum_values: &["current", "all"],
        binding_kind: None,
    },
    Extra {
        section: "keys.command",
        key: "description",
        ty: "string",
        doc: "Label for this custom command, shown wherever herdr lists it.",
        enum_values: &[],
        binding_kind: None,
    },
];

/// An enum whose members `--default-config` does not state exactly.
///
/// The form otherwise scrapes candidates out of prose, which is guesswork;
/// herdr names them precisely when it rejects a bad value, and the tests below
/// compare these lists against what it reports.
pub struct EnumOverride {
    pub section: &'static str,
    pub key: &'static str,
    pub members: &'static [&'static str],
}

pub const ENUM_OVERRIDES: &[EnumOverride] = &[EnumOverride {
    section: "keys.command",
    key: "type",
    // `--default-config` documents only the first three.
    members: &["shell", "pane", "popup", "plugin_action"],
}];

/// Popup dimensions: a percentage as a string, or a cell count as a bare
/// integer. herdr rejects `width = "120"` outright, so the form has to know
/// which of the two a value is and quote it accordingly.
pub fn is_size(section: &str, key: &str) -> bool {
    section == "keys.command" && (key == "width" || key == "height")
}

/// Settings whose value is a color, so the form can offer a picker.
///
/// herdr does not validate colors at all -- `accent = "notacolor"` passes
/// `config check` -- so the UI is the only thing standing between the user and
/// a silently ignored value.
pub fn is_color(section: &str, key: &str) -> bool {
    THEME_TABLES.contains(&section) || (section == "ui" && key == "accent")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check;

    fn probe(section: &str, key: &str) -> check::CheckReport {
        check::check_toml(&format!("[{section}]\n{key} = \"#112233\"\n"))
    }

    #[test]
    fn every_listed_token_is_accepted_by_herdr() {
        if check::check_toml("").unavailable.is_some() {
            eprintln!("herdr not installed; skipping");
            return;
        }
        for table in THEME_TABLES {
            for token in THEME_TOKENS {
                let r = probe(table, token);
                assert!(
                    r.ok,
                    "herdr rejects [{table}] {token}: {}",
                    r.raw.replace('\n', " ")
                );
            }
        }
    }

    #[test]
    fn a_name_that_is_not_a_token_is_rejected() {
        if check::check_toml("").unavailable.is_some() {
            return;
        }
        // Proves the probe above discriminates rather than passing everything.
        let r = probe("theme.custom", "definitely_not_a_token");
        assert!(!r.ok);
        assert_eq!(r.diagnostics[0].kind, check::Kind::UnknownKey);
    }

    #[test]
    fn every_extra_setting_is_accepted_by_herdr() {
        if check::check_toml("").unavailable.is_some() {
            eprintln!("herdr not installed; skipping");
            return;
        }
        for e in EXTRA_SETTINGS {
            // A value of the wrong type proves the key exists and tells us the
            // type herdr wants, which is how these were found in the first
            // place. An unknown key is reported differently.
            let r = check::check_toml(&probe_body(e.section, e.key, "0.5"));
            let unknown = r
                .diagnostics
                .iter()
                .any(|d| d.kind == check::Kind::UnknownKey);
            assert!(
                !unknown,
                "herdr does not know [{}] {}: {}",
                e.section,
                e.key,
                r.raw.replace('\n', " ")
            );
        }
    }

    #[test]
    fn extra_enum_members_match_what_herdr_reports() {
        if check::check_toml("").unavailable.is_some() {
            return;
        }
        for e in EXTRA_SETTINGS.iter().filter(|e| !e.enum_values.is_empty()) {
            let r = check::check_toml(&format!(
                "[{}]\n{} = \"definitely-not-a-member\"\n",
                e.section, e.key
            ));
            let reported = &r.diagnostics[0].allowed;
            assert_eq!(
                reported,
                &e.enum_values.to_vec(),
                "[{}] {} members drifted",
                e.section,
                e.key
            );
        }
    }

    /// `[[keys.command]]` is an array of tables, so a probe has to use the
    /// double-bracket form or herdr rejects the shape before the value.
    fn probe_body(section: &str, key: &str, value: &str) -> String {
        if section == "keys.command" {
            format!("[[keys.command]]\ncommand = \"x\"\n{key} = {value}\n")
        } else {
            format!("[{section}]\n{key} = {value}\n")
        }
    }

    #[test]
    fn enum_overrides_match_what_herdr_reports() {
        if check::check_toml("").unavailable.is_some() {
            eprintln!("herdr not installed; skipping");
            return;
        }
        for o in ENUM_OVERRIDES {
            let r = check::check_toml(&probe_body(o.section, o.key, "\"not-a-member\""));
            let reported = r
                .diagnostics
                .iter()
                .find(|d| d.kind == check::Kind::Variant)
                .map(|d| d.allowed.clone())
                .unwrap_or_else(|| {
                    panic!("no variant error for [{}] {}: {}", o.section, o.key, r.raw)
                });
            assert_eq!(
                reported,
                o.members.to_vec(),
                "[{}] {} members drifted",
                o.section,
                o.key
            );
        }
    }

    #[test]
    fn popup_sizes_are_recognised() {
        assert!(is_size("keys.command", "width"));
        assert!(is_size("keys.command", "height"));
        assert!(!is_size("keys.command", "command"));
        assert!(!is_size("ui", "sidebar_width"));
    }

    #[test]
    fn extras_do_not_duplicate_a_documented_setting() {
        let documented = crate::schema::parse(include_str!("../fixtures/default-config.toml"));
        for e in EXTRA_SETTINGS {
            let clash = documented
                .sections
                .iter()
                .any(|s| s.name == e.section && s.items.iter().any(|i| i.key == e.key));
            assert!(!clash, "[{}] {} is already documented", e.section, e.key);
        }
    }

    #[test]
    fn the_token_list_has_no_duplicates() {
        let mut sorted = THEME_TOKENS.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), before);
    }

    #[test]
    fn colors_are_recognised_only_where_they_are_colors() {
        assert!(is_color("theme.custom", "teal"));
        assert!(is_color("theme.custom.dark", "panel_bg"));
        assert!(is_color("ui", "accent"));
        assert!(!is_color("ui", "sidebar_width"));
        assert!(!is_color("theme", "name"));
    }
}
