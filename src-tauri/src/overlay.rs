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
