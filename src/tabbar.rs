//! `ui.tab_bar_right`: the status entries at the right edge of the tab bar.
//!
//! herdr documents the setting as `tab_bar_right = []` and names the five
//! types in prose, but not the fields each type takes. Those came from asking
//! `herdr config check`, which reports the expected field names verbatim:
//!
//! ```text
//! unknown field `zzz`, expected one of `command`, `interval_seconds`, `timeout_seconds`
//! ```
//!
//! | type | フィールド | 必須 |
//! | --- | --- | --- |
//! | `zoom` / `hostname` | なし | |
//! | `datetime` | `format` (string) | |
//! | `text` | `text` (string) | ✓ |
//! | `command` | `command` (string) | ✓ |
//! | | `interval_seconds` / `timeout_seconds` (u64, 1 以上) | |
//!
//! Like [`crate::rows`], parsing stays narrow: anything the editor cannot
//! represent returns None and the setting keeps its raw TOML field rather than
//! being rewritten from a half-understood structure.

use toml_edit::Value;

/// The five entry types, in the order herdr lists them.
pub const TYPES: &[&str] = &["zoom", "hostname", "datetime", "text", "command"];

/// One entry. The fields of every type are kept side by side rather than in an
/// enum so that switching the type back and forth does not discard what was
/// typed; [`to_toml`] writes only the fields the chosen type accepts.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Entry {
    pub ty: String,
    /// `datetime`. Empty means "leave it out and let herdr choose".
    pub format: String,
    /// `text`. Required by herdr, so it is always written.
    pub text: String,
    /// `command`. Required by herdr, so it is always written.
    pub command: String,
    pub interval_seconds: Option<u64>,
    pub timeout_seconds: Option<u64>,
}

pub type Entries = Vec<Entry>;

fn entry_from(value: &Value) -> Option<Entry> {
    let table = value.as_inline_table()?;
    let mut entry = Entry {
        ty: table.get("type")?.as_str()?.to_string(),
        ..Default::default()
    };
    if !TYPES.contains(&entry.ty.as_str()) {
        return None;
    }
    for (key, v) in table.iter() {
        // herdr ignores stray fields on the unit types `zoom` and `hostname`,
        // but the editor has nowhere to put them, so they fall back to raw
        // TOML rather than being silently dropped on the next save.
        match (entry.ty.as_str(), key) {
            (_, "type") => {}
            ("datetime", "format") => entry.format = v.as_str()?.to_string(),
            ("text", "text") => entry.text = v.as_str()?.to_string(),
            ("command", "command") => entry.command = v.as_str()?.to_string(),
            ("command", "interval_seconds") => {
                entry.interval_seconds = Some(v.as_integer()?.try_into().ok()?)
            }
            ("command", "timeout_seconds") => {
                entry.timeout_seconds = Some(v.as_integer()?.try_into().ok()?)
            }
            _ => return None,
        }
    }
    Some(entry)
}

pub fn parse(text: &str) -> Option<Entries> {
    let value: Value = text.trim().parse().ok()?;
    value.as_array()?.iter().map(entry_from).collect()
}

fn quote(s: &str) -> String {
    format!("{s:?}")
}

fn entry_to_toml(e: &Entry) -> String {
    let mut parts = vec![format!("type = {}", quote(&e.ty))];
    match e.ty.as_str() {
        "datetime" => {
            // An empty `format` is not the same as no `format`: herdr hides the
            // entry for the first and picks its own for the second.
            if !e.format.is_empty() {
                parts.push(format!("format = {}", quote(&e.format)));
            }
        }
        // Writing `{ type = "text" }` is a parse error, and a parse error makes
        // herdr discard the whole config. An empty string only hides this one
        // entry, so an unfinished entry is written that way instead.
        "text" => parts.push(format!("text = {}", quote(&e.text))),
        "command" => {
            parts.push(format!("command = {}", quote(&e.command)));
            if let Some(n) = e.interval_seconds {
                parts.push(format!("interval_seconds = {n}"));
            }
            if let Some(n) = e.timeout_seconds {
                parts.push(format!("timeout_seconds = {n}"));
            }
        }
        _ => {}
    }
    format!("{{ {} }}", parts.join(", "))
}

pub fn to_toml(entries: &Entries) -> String {
    let body = entries
        .iter()
        .map(entry_to_toml)
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}

/// One line of the entry, for the card header.
pub fn preview(e: &Entry) -> String {
    match e.ty.as_str() {
        "datetime" if !e.format.is_empty() => format!("datetime  {}", e.format),
        "text" => format!("text  {}", e.text),
        "command" => {
            let mut s = format!("command  {}", e.command);
            if let Some(n) = e.interval_seconds {
                s.push_str(&format!("  {n}s ごと"));
            }
            s
        }
        other => other.to_string(),
    }
}

/// What herdr will complain about, in its own terms. None of these discard the
/// config -- herdr hides the single entry and carries on -- but a hidden entry
/// looks exactly like a setting that did not take effect.
pub fn problem(e: &Entry) -> Option<String> {
    match e.ty.as_str() {
        "text" if e.text.is_empty() => Some("text が空です。この項目は表示されません".to_string()),
        "command" if e.command.trim().is_empty() => {
            Some("command が空です。この項目は表示されません".to_string())
        }
        "command" if e.interval_seconds == Some(0) => {
            Some("interval_seconds は 1 以上にしてください".to_string())
        }
        "command" if e.timeout_seconds == Some(0) => {
            Some("timeout_seconds は 1 以上にしてください".to_string())
        }
        _ => None,
    }
}

pub fn move_entry(entries: &mut Entries, index: usize, delta: isize) {
    let to = index as isize + delta;
    if to < 0 || to as usize >= entries.len() {
        return;
    }
    let e = entries.remove(index);
    entries.insert(to as usize, e);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ty: &str) -> Entry {
        Entry {
            ty: ty.into(),
            ..Default::default()
        }
    }

    #[test]
    fn every_shape_herdr_accepts_round_trips() {
        for source in [
            "[]",
            r#"[{ type = "zoom" }]"#,
            r#"[{ type = "hostname" }]"#,
            r#"[{ type = "datetime" }]"#,
            r#"[{ type = "datetime", format = "%H:%M" }]"#,
            r#"[{ type = "text", text = "|" }]"#,
            r#"[{ type = "command", command = "date" }]"#,
            r#"[{ type = "command", command = "date", interval_seconds = 10 }]"#,
            r#"[{ type = "command", command = "date", interval_seconds = 10, timeout_seconds = 2 }]"#,
            r#"[{ type = "zoom" }, { type = "hostname" }, { type = "text", text = " " }]"#,
        ] {
            let parsed = parse(source).unwrap_or_else(|| panic!("failed to parse {source}"));
            assert_eq!(to_toml(&parsed), source, "{source}");
        }
    }

    #[test]
    fn shapes_the_editor_cannot_represent_are_refused() {
        for bad in [
            // herdr's own error: `expected internally tagged enum
            // TabBarRightEntryConfig`.
            r#"["zoom"]"#,
            r#"[{ type = "notathing" }]"#,
            // Rejected by herdr: the field belongs to another type.
            r#"[{ type = "text", format = "%H" }]"#,
            r#"[{ type = "datetime", text = "x" }]"#,
            r#"[{ type = "zoom", interval_seconds = 1 }]"#,
            // Accepted by herdr but not representable here: it ignores stray
            // fields on a unit type, and dropping them on save would be silent.
            r#"[{ type = "hostname", zzz = 1 }]"#,
            // Wrong types, all of which herdr refuses.
            r#"[{ type = "text", text = 1 }]"#,
            r#"[{ type = "command", command = "date", interval_seconds = 1.5 }]"#,
            r#"[{ type = "command", command = "date", interval_seconds = -1 }]"#,
            "{ type = \"zoom\" }",
            "not an array",
        ] {
            assert!(parse(bad).is_none(), "{bad} should not parse");
        }
    }

    #[test]
    fn an_unfinished_entry_is_written_so_herdr_keeps_the_rest_of_the_config() {
        // `{ type = "text" }` is a parse error, and herdr answers a parse error
        // by throwing the entire file away.
        assert_eq!(
            to_toml(&vec![entry("text")]),
            r#"[{ type = "text", text = "" }]"#
        );
        assert_eq!(
            to_toml(&vec![entry("command")]),
            r#"[{ type = "command", command = "" }]"#
        );
    }

    #[test]
    fn fields_of_other_types_are_kept_but_not_written() {
        let e = Entry {
            ty: "zoom".into(),
            text: "typed before switching".into(),
            ..Default::default()
        };
        assert_eq!(to_toml(&vec![e.clone()]), r#"[{ type = "zoom" }]"#);
        assert_eq!(e.text, "typed before switching");
    }

    #[test]
    fn the_cases_herdr_hides_an_entry_for_are_reported() {
        assert!(problem(&entry("text")).is_some());
        assert!(problem(&entry("command")).is_some());
        assert!(problem(&Entry {
            ty: "command".into(),
            command: "date".into(),
            interval_seconds: Some(0),
            ..Default::default()
        })
        .is_some());
        assert!(problem(&entry("zoom")).is_none());
        assert!(problem(&Entry {
            ty: "command".into(),
            command: "date".into(),
            interval_seconds: Some(5),
            ..Default::default()
        })
        .is_none());
    }

    /// The shapes above were read off `herdr config check`. This asks the
    /// installed herdr whether they are still true, so the module cannot drift
    /// away from the binary in silence.
    #[test]
    fn what_this_module_writes_is_what_herdr_accepts() {
        use crate::check;
        if check::check_toml("").unavailable.is_some() {
            return; // no herdr on this machine (CI runners have none)
        }
        let config = |entries: &Entries| format!("[ui]\ntab_bar_right = {}\n", to_toml(entries));

        // Every type, filled in the way the editor fills it.
        let filled = vec![
            entry("zoom"),
            entry("hostname"),
            Entry {
                ty: "datetime".into(),
                format: "%H:%M".into(),
                ..Default::default()
            },
            Entry {
                ty: "text".into(),
                text: "|".into(),
                ..Default::default()
            },
            Entry {
                ty: "command".into(),
                command: "date".into(),
                interval_seconds: Some(10),
                timeout_seconds: Some(2),
                ..Default::default()
            },
        ];
        for e in &filled {
            let r = check::check_toml(&config(&vec![e.clone()]));
            assert!(r.ok, "herdr rejects {}: {}", e.ty, r.raw.replace('\n', " "));
        }
        let r = check::check_toml(&config(&filled));
        assert!(r.ok, "herdr rejects all five at once: {}", r.raw);

        // A type with nothing typed into it yet. herdr must still keep the
        // rest of the config, which is the whole point of writing `text = ""`.
        for ty in ["text", "command"] {
            let r = check::check_toml(&config(&vec![entry(ty)]));
            assert!(
                !r.discards_config,
                "an unfinished {ty} entry throws the config away: {}",
                r.raw.replace('\n', " ")
            );
            assert!(problem(&entry(ty)).is_some(), "{ty} should be reported");
        }

        // And the bare form really is fatal, which is why it is never written.
        let r = check::check_toml("[ui]\ntab_bar_right = [{ type = \"text\" }]\n");
        assert!(r.discards_config, "expected a fatal parse error: {}", r.raw);
    }

    #[test]
    fn entries_move_and_stop_at_the_ends() {
        let mut es = parse(r#"[{ type = "zoom" }, { type = "hostname" }]"#).unwrap();
        move_entry(&mut es, 1, -1);
        assert_eq!(
            to_toml(&es),
            r#"[{ type = "hostname" }, { type = "zoom" }]"#
        );
        move_entry(&mut es, 0, -1);
        assert_eq!(
            to_toml(&es),
            r#"[{ type = "hostname" }, { type = "zoom" }]"#
        );
    }
}
