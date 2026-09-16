//! Reading and writing the user's config.toml.
//!
//! Three-state model for every setting:
//!   key absent      -> inheriting herdr's default
//!   key present     -> explicitly set
//!   key present ""  -> explicitly disabled
//!
//! Writes go through toml_edit so comments, blank lines, key order and the
//! decor of untouched settings all survive. Only the edited keys are touched;
//! a setting returned to "inherit" is removed from the file rather than
//! written out as its default value.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::herdr;

/// `HERDR_GUI_CONFIG` overrides the target file. Used by tests so they never
/// touch the real config.
pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("HERDR_GUI_CONFIG") {
        return Some(PathBuf::from(p));
    }
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("herdr").join("config.toml"))
    } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        Some(PathBuf::from(xdg).join("herdr").join("config.toml"))
    } else {
        herdr::home_dir().map(|h| h.join(".config").join("herdr").join("config.toml"))
    }
}

#[derive(Serialize)]
pub struct ConfigState {
    pub path: String,
    pub exists: bool,
    pub raw: String,
    /// Dotted paths of every leaf value present in the file.
    pub set_paths: Vec<String>,
    /// path -> value, as verbatim TOML source text.
    pub values: std::collections::BTreeMap<String, String>,
    pub parse_error: Option<String>,
}

/// One requested change. `value` is verbatim TOML source text ("true", "42",
/// "\"ctrl+a\"", "[\"a\", \"b\"]"); `None` means "remove, inherit the default".
#[derive(Deserialize, Clone, Debug)]
pub struct Edit {
    pub path: String,
    pub value: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Change {
    pub path: String,
    pub from: Option<String>,
    pub to: Option<String>,
    /// `add` | `update` | `remove` | `noop`
    pub action: &'static str,
}

#[derive(Serialize)]
pub struct Preview {
    pub changes: Vec<Change>,
    pub after: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SaveResult {
    pub path: String,
    pub backup: Option<String>,
    pub changes: Vec<Change>,
    pub check_ok: bool,
    pub check_output: String,
    pub reloaded: Option<bool>,
    pub reload_output: String,
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() { key.to_string() } else { format!("{prefix}.{key}") }
}

fn walk(item: &Item, prefix: &str, out: &mut Vec<(String, String)>) {
    match item {
        Item::Table(t) => {
            for (k, v) in t.iter() {
                walk(v, &join(prefix, k), out);
            }
        }
        Item::Value(Value::InlineTable(t)) => {
            for (k, v) in t.iter() {
                out.push((join(prefix, k), v.to_string().trim().to_string()));
            }
        }
        Item::Value(v) => out.push((prefix.to_string(), v.to_string().trim().to_string())),
        Item::ArrayOfTables(arr) => {
            for (i, t) in arr.iter().enumerate() {
                for (k, v) in t.iter() {
                    walk(v, &join(&format!("{prefix}[{i}]"), k), out);
                }
            }
        }
        Item::None => {}
    }
}

fn leaves(doc: &DocumentMut) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (k, v) in doc.as_table().iter() {
        walk(v, k, &mut out);
    }
    out
}

pub fn load() -> ConfigState {
    let path = config_path();
    let display = path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unresolved>".into());

    let empty = |exists, raw, err| ConfigState {
        path: display.clone(),
        exists,
        raw,
        set_paths: Vec::new(),
        values: Default::default(),
        parse_error: err,
    };

    let Some(path) = path else {
        return empty(false, String::new(), Some("could not resolve a config directory".into()));
    };
    if !path.is_file() {
        return empty(false, String::new(), None);
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return empty(true, String::new(), Some(e.to_string())),
    };
    match raw.parse::<DocumentMut>() {
        Ok(doc) => {
            let pairs = leaves(&doc);
            ConfigState {
                path: display,
                exists: true,
                raw,
                set_paths: pairs.iter().map(|(p, _)| p.clone()).collect(),
                values: pairs.into_iter().collect(),
                parse_error: None,
            }
        }
        Err(e) => empty(true, raw, Some(e.to_string())),
    }
}

/// Descend to the table owning `key`, creating missing intermediate tables as
/// implicit so no empty `[ui]` / `[ui.sidebar]` headers get emitted.
fn owner_table<'a>(doc: &'a mut DocumentMut, parts: &[&str]) -> Result<&'a mut Table, String> {
    let mut tbl = doc.as_table_mut();
    for (i, part) in parts.iter().enumerate() {
        if !tbl.contains_key(part) {
            let mut new = Table::new();
            new.set_implicit(true);
            tbl.insert(part, Item::Table(new));
        }
        tbl = tbl
            .get_mut(part)
            .and_then(|it| it.as_table_mut())
            .ok_or_else(|| format!("{} is not a table", parts[..=i].join(".")))?;
    }
    // The innermost table must be rendered, otherwise its keys have nowhere to go.
    tbl.set_implicit(false);
    Ok(tbl)
}

fn prune(tbl: &mut Table) {
    let empties: Vec<String> = tbl
        .iter()
        .filter_map(|(k, v)| match v {
            Item::Table(t) if t.is_empty() => Some(k.to_string()),
            _ => None,
        })
        .collect();
    for k in empties {
        tbl.remove(&k);
    }
    for (_, v) in tbl.iter_mut() {
        if let Item::Table(t) = v {
            prune(t);
        }
    }
}

pub fn apply_edits(raw: &str, edits: &[Edit]) -> Result<(String, Vec<Change>), String> {
    let mut doc: DocumentMut = raw.parse().map_err(|e| format!("config.toml is not valid TOML: {e}"))?;
    let before: std::collections::BTreeMap<String, String> = leaves(&doc).into_iter().collect();
    let mut changes = Vec::new();

    for edit in edits {
        let parts: Vec<&str> = edit.path.split('.').collect();
        let (key, parents) = parts.split_last().ok_or("empty path")?;
        let from = before.get(&edit.path).cloned();

        match &edit.value {
            Some(text) => {
                let parsed: Value = text
                    .parse()
                    .map_err(|e| format!("{}: not a valid TOML value ({text}): {e}", edit.path))?;
                let normalized = parsed.to_string().trim().to_string();
                if from.as_deref() == Some(normalized.as_str()) {
                    changes.push(Change { path: edit.path.clone(), from, to: Some(normalized), action: "noop" });
                    continue;
                }
                let tbl = owner_table(&mut doc, parents)?;
                match tbl.get_mut(key) {
                    // Replace in place so a trailing `# comment` survives.
                    Some(Item::Value(old)) => {
                        let decor = old.decor().clone();
                        let mut next = parsed;
                        *next.decor_mut() = decor;
                        *old = next;
                    }
                    _ => {
                        tbl.insert(key, Item::Value(parsed));
                    }
                }
                changes.push(Change {
                    path: edit.path.clone(),
                    action: if from.is_some() { "update" } else { "add" },
                    from,
                    to: Some(normalized),
                });
            }
            None => {
                if from.is_none() {
                    changes.push(Change { path: edit.path.clone(), from: None, to: None, action: "noop" });
                    continue;
                }
                let tbl = owner_table(&mut doc, parents)?;
                tbl.remove(key);
                changes.push(Change { path: edit.path.clone(), from, to: None, action: "remove" });
            }
        }
    }

    prune(doc.as_table_mut());
    Ok((doc.to_string(), changes))
}

pub fn preview(edits: Vec<Edit>) -> Preview {
    let state = load();
    match apply_edits(&state.raw, &edits) {
        Ok((after, changes)) => Preview { changes, after, error: None },
        Err(e) => Preview { changes: Vec::new(), after: state.raw, error: Some(e) },
    }
}

pub fn save(edits: Vec<Edit>, reload: bool) -> Result<SaveResult, String> {
    let path = config_path().ok_or("could not resolve a config directory")?;
    let raw = if path.is_file() { std::fs::read_to_string(&path).map_err(|e| e.to_string())? } else { String::new() };
    let (after, changes) = apply_edits(&raw, &edits)?;

    if changes.iter().all(|c| c.action == "noop") {
        return Ok(SaveResult {
            path: path.display().to_string(),
            backup: None,
            changes,
            check_ok: true,
            check_output: "no changes to write".into(),
            reloaded: None,
            reload_output: String::new(),
        });
    }

    // Always keep the previous revision before overwriting.
    let backup = if path.is_file() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let b = path.with_extension(format!("toml.bak-{stamp}"));
        std::fs::copy(&path, &b).map_err(|e| format!("backup failed: {e}"))?;
        Some(b.display().to_string())
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        None
    };

    std::fs::write(&path, &after).map_err(|e| format!("write failed: {e}"))?;

    let (check_ok, check_output) = match herdr::run(&["config", "check"]) {
        Ok(o) => (true, o.trim().to_string()),
        Err(e) => (false, e),
    };

    let (reloaded, reload_output) = if reload && check_ok {
        match herdr::run(&["server", "reload-config"]) {
            Ok(o) => (Some(true), o.trim().to_string()),
            Err(e) => (Some(false), e),
        }
    } else {
        (None, String::new())
    };

    Ok(SaveResult {
        path: path.display().to_string(),
        backup,
        changes,
        check_ok,
        check_output,
        reloaded,
        reload_output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"onboarding = false

[ui]
status_indicators = "dots"   # keep the compact marks

[theme]
name = "terminal"
auto_switch = false

[keys]
prefix = "ctrl+a"
split_vertical = "prefix+|"
"#;

    fn edit(path: &str, value: Option<&str>) -> Edit {
        Edit { path: path.into(), value: value.map(|s| s.to_string()) }
    }

    #[test]
    fn updates_only_the_edited_line() {
        let (after, changes) = apply_edits(SAMPLE, &[edit("keys.prefix", Some("\"ctrl+b\""))]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].action, "update");
        assert_eq!(changes[0].from.as_deref(), Some("\"ctrl+a\""));
        assert!(after.contains("prefix = \"ctrl+b\""));
        // Everything else is byte-identical.
        assert_eq!(
            after.replace("prefix = \"ctrl+b\"", "prefix = \"ctrl+a\""),
            SAMPLE
        );
    }

    #[test]
    fn preserves_trailing_comments_on_edit() {
        let (after, _) = apply_edits(SAMPLE, &[edit("ui.status_indicators", Some("\"symbols\""))]).unwrap();
        assert!(
            after.contains("status_indicators = \"symbols\"   # keep the compact marks"),
            "trailing comment lost:\n{after}"
        );
    }

    #[test]
    fn returning_to_default_removes_the_key() {
        let (after, changes) = apply_edits(SAMPLE, &[edit("theme.auto_switch", None)]).unwrap();
        assert_eq!(changes[0].action, "remove");
        assert!(!after.contains("auto_switch"));
        assert!(after.contains("name = \"terminal\""), "sibling key dropped");
    }

    #[test]
    fn creates_nested_tables_without_empty_headers() {
        let (after, changes) =
            apply_edits(SAMPLE, &[edit("ui.sidebar.agents.row_gap", Some("1"))]).unwrap();
        assert_eq!(changes[0].action, "add");
        assert!(after.contains("[ui.sidebar.agents]"), "missing leaf header:\n{after}");
        assert!(!after.contains("[ui.sidebar]\n"), "emitted empty parent header:\n{after}");
        assert!(after.contains("row_gap = 1"));
    }

    #[test]
    fn removing_the_last_key_prunes_the_table() {
        let (after, _) = apply_edits(
            SAMPLE,
            &[edit("keys.prefix", None), edit("keys.split_vertical", None)],
        )
        .unwrap();
        assert!(!after.contains("[keys]"), "empty table left behind:\n{after}");
        assert!(after.contains("[theme]"));
    }

    #[test]
    fn rejects_invalid_values_before_touching_the_document() {
        let err = apply_edits(SAMPLE, &[edit("server.headless_cols", Some("not a number"))]).unwrap_err();
        assert!(err.contains("not a valid TOML value"), "{err}");
    }

    #[test]
    fn writing_the_same_value_is_a_noop() {
        let (after, changes) = apply_edits(SAMPLE, &[edit("theme.name", Some("\"terminal\""))]).unwrap();
        assert_eq!(changes[0].action, "noop");
        assert_eq!(after, SAMPLE);
    }

    #[test]
    fn empty_string_is_an_explicit_disable_not_a_removal() {
        let (after, changes) = apply_edits(SAMPLE, &[edit("keys.split_vertical", Some("\"\""))]).unwrap();
        assert_eq!(changes[0].action, "update");
        assert!(after.contains("split_vertical = \"\""));
    }
}

/// Exercises the real write path (backup, format-preserving write, `herdr
/// config check`) against a throwaway file via `HERDR_GUI_CONFIG`.
///
/// Runs single-threaded because it mutates a process-wide env var; the other
/// tests in this module call `apply_edits` directly and never read it.
#[cfg(test)]
mod save_tests {
    use super::*;

    #[test]
    fn save_writes_minimally_and_keeps_a_backup() {
        let dir = std::env::temp_dir().join(format!("herdr-gui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        let original = "onboarding = false\n\n[theme]\nname = \"terminal\"\nauto_switch = false\n";
        std::fs::write(&file, original).unwrap();
        std::env::set_var("HERDR_GUI_CONFIG", &file);

        let result = save(
            vec![
                Edit { path: "theme.name".into(), value: Some("\"kanagawa\"".into()) },
                Edit { path: "theme.auto_switch".into(), value: None },
                Edit { path: "ui.sidebar_width".into(), value: Some("30".into()) },
            ],
            false,
        )
        .expect("save");

        let after = std::fs::read_to_string(&file).unwrap();
        assert!(after.contains("name = \"kanagawa\""));
        assert!(!after.contains("auto_switch"), "removed key still present:\n{after}");
        assert!(after.contains("[ui]") && after.contains("sidebar_width = 30"), "{after}");
        assert!(after.starts_with("onboarding = false\n"), "untouched head changed:\n{after}");

        let actions: Vec<&str> = result.changes.iter().map(|c| c.action).collect();
        assert_eq!(actions, ["update", "remove", "add"]);

        let backup = result.backup.expect("backup path");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert!(result.reloaded.is_none(), "reload must not run when not requested");

        std::env::remove_var("HERDR_GUI_CONFIG");
        std::fs::remove_dir_all(&dir).ok();
    }
}
