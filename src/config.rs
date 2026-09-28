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
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

use crate::check::{self, CheckReport};
use crate::herdr;

/// Line endings of the file being edited.
///
/// toml_edit normalizes every newline to LF when it renders a document, which
/// would rewrite every line of a CRLF file and turn a one-setting change into
/// a whole-file diff. The original style is detected on read and restored on
/// write.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Newline {
    Lf,
    Crlf,
}

impl Newline {
    /// A file written on Windows uses CRLF throughout, so one occurrence is
    /// enough. A mixed file is normalized to CRLF rather than left ragged.
    pub fn detect(raw: &str) -> Newline {
        if raw.contains("\r\n") {
            Newline::Crlf
        } else {
            Newline::Lf
        }
    }

    pub fn apply(self, s: &str) -> String {
        match self {
            Newline::Lf => s.to_string(),
            // Collapse first so an already-CRLF newline cannot become CR CR LF.
            Newline::Crlf => s.replace("\r\n", "\n").replace('\n', "\r\n"),
        }
    }
}

/// The environment that decides where the config lives. Kept separate from the
/// process so the per-platform rules can be tested on any host.
#[derive(Clone, Default, Debug)]
pub struct PathEnv {
    pub windows: bool,
    /// `HERDR_CONFIG_PATH`, herdr's own override.
    pub override_path: Option<PathBuf>,
    /// `%APPDATA%`, Windows only.
    pub appdata: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl PathEnv {
    pub fn from_process() -> Self {
        Self {
            windows: cfg!(windows),
            override_path: std::env::var_os("HERDR_CONFIG_PATH").map(PathBuf::from),
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home: herdr::home_dir(),
        }
    }
}

/// Fallback for when herdr cannot be asked. Mirrors the rules herdr documents:
/// `%APPDATA%\herdr\config.toml` on Windows, `$XDG_CONFIG_HOME/herdr` when
/// set, otherwise `~/.config/herdr`.
pub fn resolve_config_path(env: &PathEnv) -> Option<PathBuf> {
    if let Some(p) = &env.override_path {
        return Some(p.clone());
    }
    let dir = if env.windows {
        env.appdata
            .clone()
            .or_else(|| env.home.as_ref().map(|h| h.join("AppData").join("Roaming")))?
    } else if let Some(xdg) = &env.xdg_config_home {
        xdg.clone()
    } else {
        env.home.as_ref()?.join(".config")
    };
    Some(dir.join("herdr").join("config.toml"))
}

/// The file herdr itself reads.
///
/// herdr prints its resolved path in `--help`, so we ask instead of guessing:
/// that already accounts for `HERDR_CONFIG_PATH`, `XDG_CONFIG_HOME` and the
/// Windows layout. The computed fallback only matters when herdr is missing.
pub fn config_path() -> Option<PathBuf> {
    herdr::resolved_config_path().or_else(|| resolve_config_path(&PathEnv::from_process()))
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

/// What an edit does.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Write `value`, or remove the key when it is `None`.
    #[default]
    Set,
    /// Delete a whole array-of-tables entry, e.g. `keys.command[1]`.
    RemoveEntry,
}

/// One requested change. `value` is verbatim TOML source text ("true", "42",
/// "\"ctrl+a\"", "[\"a\", \"b\"]"); `None` means "remove, inherit the default".
///
/// Paths may index an array of tables: `keys.command[0].key`.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Edit {
    pub path: String,
    pub value: Option<String>,
    #[serde(default)]
    pub op: Op,
}

/// One step of a dotted path. `[[keys.command]]` entries are addressed by
/// index, so a segment is either a plain key or an indexed entry.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Key(String),
    Entry(String, usize),
}

pub fn parse_path(path: &str) -> Result<Vec<Seg>, String> {
    let mut out = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            return Err(format!("{path}: empty path segment"));
        }
        match part.find('[') {
            Some(open) => {
                let name = &part[..open];
                if name.is_empty() || !part.ends_with(']') {
                    return Err(format!("{path}: malformed index in {part}"));
                }
                let index = part[open + 1..part.len() - 1]
                    .parse::<usize>()
                    .map_err(|_| format!("{path}: {part} has a non-numeric index"))?;
                out.push(Seg::Entry(name.to_string(), index));
            }
            None => out.push(Seg::Key(part.to_string())),
        }
    }
    Ok(out)
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
pub struct SaveResult {
    pub path: String,
    /// False when the pre-flight check refused the content.
    pub written: bool,
    pub backup: Option<String>,
    pub changes: Vec<Change>,
    pub check: CheckReport,
    pub reloaded: Option<bool>,
    pub reload_output: String,
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
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
        return empty(
            false,
            String::new(),
            Some("could not resolve a config directory".into()),
        );
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

/// Descend to the table that owns the leaf, creating what is missing.
///
/// Intermediate tables are created implicit so no empty `[ui]` /
/// `[ui.sidebar]` headers get emitted; the innermost one is made explicit
/// because its keys need a header to live under. A new array-of-tables entry
/// may only be appended, never created at an arbitrary index, so the file
/// cannot end up with blank entries padding a gap.
fn owner_table<'a>(doc: &'a mut DocumentMut, segs: &[Seg]) -> Result<&'a mut Table, String> {
    let mut tbl = doc.as_table_mut();
    let mut ends_in_table = false;

    for seg in segs {
        match seg {
            Seg::Key(name) => {
                if !tbl.contains_key(name) {
                    let mut new = Table::new();
                    new.set_implicit(true);
                    tbl.insert(name, Item::Table(new));
                }
                tbl = tbl
                    .get_mut(name)
                    .and_then(|it| it.as_table_mut())
                    .ok_or_else(|| format!("{name} is not a table"))?;
                ends_in_table = true;
            }
            Seg::Entry(name, index) => {
                if !tbl.contains_key(name) {
                    tbl.insert(name, Item::ArrayOfTables(ArrayOfTables::new()));
                }
                let aot = tbl
                    .get_mut(name)
                    .and_then(|it| it.as_array_of_tables_mut())
                    .ok_or_else(|| format!("{name} is not an array of tables"))?;
                if *index > aot.len() {
                    return Err(format!(
                        "{name}[{index}] cannot be created: only {} entries exist",
                        aot.len()
                    ));
                }
                if *index == aot.len() {
                    aot.push(Table::new());
                }
                tbl = aot
                    .get_mut(*index)
                    .ok_or_else(|| format!("{name}[{index}] is missing"))?;
                ends_in_table = false;
            }
        }
    }

    if ends_in_table {
        tbl.set_implicit(false);
    }
    Ok(tbl)
}

fn prune(tbl: &mut Table) {
    let empties: Vec<String> = tbl
        .iter()
        .filter_map(|(k, v)| match v {
            Item::Table(t) if t.is_empty() => Some(k.to_string()),
            // An array of tables whose entries are all gone, or which only
            // holds blank entries, leaves a stray `[[...]]` header behind.
            Item::ArrayOfTables(a) if a.is_empty() || a.iter().all(|t| t.is_empty()) => {
                Some(k.to_string())
            }
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
    let newline = Newline::detect(raw);
    let mut doc: DocumentMut = raw
        .parse()
        .map_err(|e| format!("config.toml is not valid TOML: {e}"))?;
    let before: std::collections::BTreeMap<String, String> = leaves(&doc).into_iter().collect();
    let mut changes = Vec::new();

    // Entry deletions run last, highest index first: removing an entry shifts
    // the ones after it, so any other order would delete the wrong rows.
    let mut removals: Vec<&Edit> = edits.iter().filter(|e| e.op == Op::RemoveEntry).collect();
    removals.sort_by_key(|e| std::cmp::Reverse(entry_index(&e.path).unwrap_or(0)));

    for edit in edits.iter().filter(|e| e.op == Op::Set) {
        let segs = parse_path(&edit.path)?;
        let (leaf, parents) = segs.split_last().ok_or("empty path")?;
        let Seg::Key(key) = leaf else {
            return Err(format!(
                "{}: a value cannot be written to an entry",
                edit.path
            ));
        };
        let from = before.get(&edit.path).cloned();

        match &edit.value {
            Some(text) => {
                let parsed: Value = text
                    .parse()
                    .map_err(|e| format!("{}: not a valid TOML value ({text}): {e}", edit.path))?;
                let normalized = parsed.to_string().trim().to_string();
                if from.as_deref() == Some(normalized.as_str()) {
                    changes.push(Change {
                        path: edit.path.clone(),
                        from,
                        to: Some(normalized),
                        action: "noop",
                    });
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
                    changes.push(Change {
                        path: edit.path.clone(),
                        from: None,
                        to: None,
                        action: "noop",
                    });
                    continue;
                }
                let tbl = owner_table(&mut doc, parents)?;
                tbl.remove(key);
                changes.push(Change {
                    path: edit.path.clone(),
                    from,
                    to: None,
                    action: "remove",
                });
            }
        }
    }

    for edit in removals {
        let segs = parse_path(&edit.path)?;
        let (leaf, parents) = segs.split_last().ok_or("empty path")?;
        let Seg::Entry(name, index) = leaf else {
            return Err(format!("{}: remove_entry needs an indexed path", edit.path));
        };
        let noop = Change {
            path: edit.path.clone(),
            from: None,
            to: None,
            action: "noop",
        };
        let Some(tbl) = find_table(&mut doc, parents) else {
            changes.push(noop);
            continue;
        };
        let Some(aot) = tbl.get_mut(name).and_then(|it| it.as_array_of_tables_mut()) else {
            changes.push(noop);
            continue;
        };
        if *index >= aot.len() {
            changes.push(noop);
            continue;
        }
        aot.remove(*index);
        if aot.is_empty() {
            tbl.remove(name);
        }
        changes.push(Change {
            path: edit.path.clone(),
            from: Some("(entry)".into()),
            to: None,
            action: "remove",
        });
    }

    prune(doc.as_table_mut());
    Ok((newline.apply(&doc.to_string()), changes))
}

/// Navigate without creating anything. Removing from a table that does not
/// exist must not bring it into being: `owner_table` would materialize an
/// empty `[keys]` header on the way to a `keys.command[9]` that is not there.
fn find_table<'a>(doc: &'a mut DocumentMut, segs: &[Seg]) -> Option<&'a mut Table> {
    let mut tbl = doc.as_table_mut();
    for seg in segs {
        tbl = match seg {
            Seg::Key(name) => tbl.get_mut(name)?.as_table_mut()?,
            Seg::Entry(name, index) => tbl
                .get_mut(name)?
                .as_array_of_tables_mut()?
                .get_mut(*index)?,
        };
    }
    Some(tbl)
}

/// Index of the last segment, when the path ends in one.
fn entry_index(path: &str) -> Option<usize> {
    match parse_path(path).ok()?.pop()? {
        Seg::Entry(_, i) => Some(i),
        Seg::Key(_) => None,
    }
}

pub fn save(edits: Vec<Edit>, reload: bool) -> Result<SaveResult, String> {
    let path = config_path().ok_or("could not resolve a config directory")?;
    let raw = if path.is_file() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let (after, changes) = apply_edits(&raw, &edits)?;

    let nothing_to_do = changes.iter().all(|c| c.action == "noop");
    if nothing_to_do {
        return Ok(SaveResult {
            path: path.display().to_string(),
            written: false,
            backup: None,
            changes,
            check: check::parse_output("config: ok"),
            reloaded: None,
            reload_output: "no changes to write".into(),
        });
    }

    // Ask herdr about the exact bytes we are about to write. A type or syntax
    // error would make herdr discard the whole file, silently reverting every
    // setting the user has, so that must never reach disk.
    let check = check::check_toml(&after);
    if check.fatal() {
        return Ok(SaveResult {
            path: path.display().to_string(),
            written: false,
            backup: None,
            changes,
            check,
            reloaded: None,
            reload_output: String::new(),
        });
    }

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

    let (reloaded, reload_output) = if reload {
        match herdr::run(&["server", "reload-config"]) {
            Ok(o) => (Some(true), o.trim().to_string()),
            Err(e) => (Some(false), e),
        }
    } else {
        (None, String::new())
    };

    Ok(SaveResult {
        path: path.display().to_string(),
        written: true,
        backup,
        changes,
        check,
        reloaded,
        reload_output,
    })
}

/// Exercises the real write path (backup, format-preserving write, `herdr
/// config check`) against a throwaway file via `HERDR_CONFIG_PATH`.
///
/// That is herdr's own override rather than one of ours, so the `herdr config
/// check` this triggers validates the very file we wrote.
///
/// Runs single-threaded because it mutates a process-wide env var; the other
/// tests in this module call `apply_edits` directly and never read it.
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
        Edit {
            path: path.into(),
            value: value.map(|s| s.to_string()),
            op: Op::Set,
        }
    }

    #[test]
    fn updates_only_the_edited_line() {
        let (after, changes) =
            apply_edits(SAMPLE, &[edit("keys.prefix", Some("\"ctrl+b\""))]).unwrap();
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
        let (after, _) =
            apply_edits(SAMPLE, &[edit("ui.status_indicators", Some("\"symbols\""))]).unwrap();
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
        assert!(
            after.contains("[ui.sidebar.agents]"),
            "missing leaf header:\n{after}"
        );
        assert!(
            !after.contains("[ui.sidebar]\n"),
            "emitted empty parent header:\n{after}"
        );
        assert!(after.contains("row_gap = 1"));
    }

    #[test]
    fn removing_the_last_key_prunes_the_table() {
        let (after, _) = apply_edits(
            SAMPLE,
            &[edit("keys.prefix", None), edit("keys.split_vertical", None)],
        )
        .unwrap();
        assert!(
            !after.contains("[keys]"),
            "empty table left behind:\n{after}"
        );
        assert!(after.contains("[theme]"));
    }

    #[test]
    fn rejects_invalid_values_before_touching_the_document() {
        let err = apply_edits(
            SAMPLE,
            &[edit("server.headless_cols", Some("not a number"))],
        )
        .unwrap_err();
        assert!(err.contains("not a valid TOML value"), "{err}");
    }

    #[test]
    fn writing_the_same_value_is_a_noop() {
        let (after, changes) =
            apply_edits(SAMPLE, &[edit("theme.name", Some("\"terminal\""))]).unwrap();
        assert_eq!(changes[0].action, "noop");
        assert_eq!(after, SAMPLE);
    }

    #[test]
    fn empty_string_is_an_explicit_disable_not_a_removal() {
        let (after, changes) =
            apply_edits(SAMPLE, &[edit("keys.split_vertical", Some("\"\""))]).unwrap();
        assert_eq!(changes[0].action, "update");
        assert!(after.contains("split_vertical = \"\""));
    }
}

#[cfg(test)]
mod aot_tests {
    use super::*;

    const TWO: &str = "\
[[keys.command]]
key = \"prefix+alt+g\"
type = \"popup\"
command = \"lazygit\"   # my git popup

[[keys.command]]
key = \"prefix+alt+t\"
type = \"shell\"
command = \"echo hi\"
";

    fn set(path: &str, value: &str) -> Edit {
        Edit {
            path: path.into(),
            value: Some(value.to_string()),
            op: Op::Set,
        }
    }
    fn drop_entry(path: &str) -> Edit {
        Edit {
            path: path.into(),
            value: None,
            op: Op::RemoveEntry,
        }
    }

    #[test]
    fn paths_may_index_an_entry() {
        assert_eq!(
            parse_path("keys.command[0].key").unwrap(),
            vec![
                Seg::Key("keys".into()),
                Seg::Entry("command".into(), 0),
                Seg::Key("key".into()),
            ]
        );
        assert_eq!(
            parse_path("theme.name").unwrap(),
            vec![Seg::Key("theme".into()), Seg::Key("name".into())]
        );
        for bad in [
            "keys.command[x].key",
            "keys.command[0.key",
            "keys..name",
            "keys.[0]",
        ] {
            assert!(parse_path(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn the_first_entry_is_written_as_an_array_of_tables() {
        // Writing it as a plain `[keys.command]` table makes herdr discard the
        // whole config, so the double-bracket form matters.
        let (after, changes) = apply_edits(
            "",
            &[
                set("keys.command[0].key", "\"prefix+alt+g\""),
                set("keys.command[0].type", "\"popup\""),
                set("keys.command[0].command", "\"lazygit\""),
            ],
        )
        .unwrap();
        assert!(after.contains("[[keys.command]]"), "{after}");
        assert!(!after.contains("\n[keys.command]"), "{after}");
        assert_eq!(changes.iter().filter(|c| c.action == "add").count(), 3);
    }

    #[test]
    fn a_second_entry_is_appended_without_touching_the_first() {
        let (after, _) = apply_edits(
            TWO,
            &[
                set("keys.command[2].key", "\"prefix+alt+d\""),
                set("keys.command[2].command", "\"btop\""),
            ],
        )
        .unwrap();
        assert_eq!(after.matches("[[keys.command]]").count(), 3);
        assert!(
            after.contains("command = \"lazygit\"   # my git popup"),
            "{after}"
        );
        assert!(after.contains("prefix+alt+d"));
    }

    #[test]
    fn an_entry_cannot_be_created_past_the_end() {
        // Otherwise the file would gain blank entries padding the gap.
        let err = apply_edits(TWO, &[set("keys.command[5].key", "\"prefix+x\"")]).unwrap_err();
        assert!(err.contains("only 2 entries exist"), "{err}");
    }

    #[test]
    fn removing_an_entry_keeps_the_others_verbatim() {
        let (after, changes) = apply_edits(TWO, &[drop_entry("keys.command[0]")]).unwrap();
        assert_eq!(after.matches("[[keys.command]]").count(), 1);
        assert!(!after.contains("lazygit"));
        assert!(after.contains("key = \"prefix+alt+t\""));
        assert_eq!(changes[0].action, "remove");
    }

    #[test]
    fn removing_the_last_entry_removes_the_header_too() {
        let (after, _) = apply_edits(
            TWO,
            &[drop_entry("keys.command[0]"), drop_entry("keys.command[1]")],
        )
        .unwrap();
        assert!(
            !after.contains("keys.command"),
            "stray header left: {after:?}"
        );
    }

    #[test]
    fn several_removals_in_one_batch_delete_the_intended_rows() {
        let three =
            format!("{TWO}\n[[keys.command]]\nkey = \"prefix+alt+d\"\ncommand = \"btop\"\n");
        // Deleting 0 first would shift 2 down to 1; the highest index must go
        // first for both to be the rows the user picked.
        let (after, _) = apply_edits(
            &three,
            &[drop_entry("keys.command[0]"), drop_entry("keys.command[2]")],
        )
        .unwrap();
        assert_eq!(after.matches("[[keys.command]]").count(), 1);
        assert!(
            after.contains("prefix+alt+t"),
            "the middle entry must survive: {after}"
        );
        assert!(!after.contains("lazygit"));
        assert!(!after.contains("btop"));
    }

    #[test]
    fn removing_a_missing_entry_is_a_noop() {
        let (after, changes) = apply_edits(TWO, &[drop_entry("keys.command[9]")]).unwrap();
        assert_eq!(after, TWO);
        assert_eq!(changes[0].action, "noop");
    }

    #[test]
    fn popup_sizes_keep_their_toml_type() {
        // A percentage is a string; a cell count is a bare integer. herdr
        // rejects `width = "120"` outright.
        let (after, _) = apply_edits(
            TWO,
            &[
                set("keys.command[0].width", "\"80%\""),
                set("keys.command[1].width", "120"),
            ],
        )
        .unwrap();
        assert!(after.contains("width = \"80%\""), "{after}");
        assert!(after.contains("width = 120"), "{after}");
    }

    #[test]
    fn a_value_cannot_be_written_onto_an_entry_itself() {
        let err = apply_edits(TWO, &[set("keys.command[0]", "\"x\"")]).unwrap_err();
        assert!(err.contains("cannot be written to an entry"), "{err}");
    }

    /// The whole point of the indexed paths: what the form builds must be
    /// something herdr accepts. Skipped when herdr is unavailable.
    #[test]
    fn what_the_form_builds_is_accepted_by_herdr() {
        if check::check_toml("").unavailable.is_some() {
            eprintln!("herdr not installed; skipping");
            return;
        }

        let (one, _) = apply_edits(
            "",
            &[
                set("keys.command[0].key", "\"prefix+alt+g\""),
                set("keys.command[0].type", "\"popup\""),
                set("keys.command[0].command", "\"lazygit\""),
                set("keys.command[0].width", "\"80%\""),
                set("keys.command[0].height", "\"80%\""),
                set("keys.command[0].description", "\"Git\""),
            ],
        )
        .unwrap();
        let r = check::check_toml(&one);
        assert!(r.ok, "first entry rejected: {}", r.raw);

        let (two, _) = apply_edits(
            &one,
            &[
                set("keys.command[1].key", "\"prefix+alt+t\""),
                set("keys.command[1].type", "\"shell\""),
                set("keys.command[1].command", "\"echo hi\""),
            ],
        )
        .unwrap();
        let r = check::check_toml(&two);
        assert!(r.ok, "second entry rejected: {}", r.raw);

        let (left, _) = apply_edits(&two, &[drop_entry("keys.command[0]")]).unwrap();
        let r = check::check_toml(&left);
        assert!(r.ok, "config after a deletion rejected: {}", r.raw);
        assert!(left.contains("echo hi"));
        assert!(!left.contains("lazygit"));
    }

    /// Why the indexed paths exist at all: the single-table form herdr is
    /// given by a naive writer is not merely ignored, it discards everything.
    #[test]
    fn the_single_table_form_would_discard_the_whole_config() {
        if check::check_toml("").unavailable.is_some() {
            return;
        }
        let r = check::check_toml("[keys.command]\nkey = \"prefix+alt+g\"\ncommand = \"x\"\n");
        assert!(r.fatal());
        assert!(r.discards_config);
    }

    #[test]
    fn entries_are_reported_by_index_when_read_back() {
        let state = apply_edits(TWO, &[]).unwrap().0;
        let doc: DocumentMut = state.parse().unwrap();
        let paths: Vec<String> = leaves(&doc).into_iter().map(|(p, _)| p).collect();
        assert!(
            paths.contains(&"keys.command[0].key".to_string()),
            "{paths:?}"
        );
        assert!(
            paths.contains(&"keys.command[1].command".to_string()),
            "{paths:?}"
        );
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    fn env() -> PathEnv {
        PathEnv {
            windows: false,
            ..Default::default()
        }
    }

    #[test]
    fn windows_uses_appdata() {
        let e = PathEnv {
            windows: true,
            appdata: Some(PathBuf::from("C:/Users/me/AppData/Roaming")),
            home: Some(PathBuf::from("C:/Users/me")),
            ..env()
        };
        assert_eq!(
            resolve_config_path(&e),
            Some(
                PathBuf::from("C:/Users/me/AppData/Roaming")
                    .join("herdr")
                    .join("config.toml")
            )
        );
    }

    #[test]
    fn windows_falls_back_to_the_profile_when_appdata_is_unset() {
        let e = PathEnv {
            windows: true,
            home: Some(PathBuf::from("C:/Users/me")),
            ..env()
        };
        assert_eq!(
            resolve_config_path(&e),
            Some(
                PathBuf::from("C:/Users/me")
                    .join("AppData")
                    .join("Roaming")
                    .join("herdr")
                    .join("config.toml")
            )
        );
    }

    #[test]
    fn unix_honors_xdg_config_home() {
        // Verified against the installed herdr: setting XDG_CONFIG_HOME moves
        // the path it reports, so we must follow it or edit the wrong file.
        let e = PathEnv {
            xdg_config_home: Some(PathBuf::from("/tmp/xdg")),
            home: Some(PathBuf::from("/home/me")),
            ..env()
        };
        assert_eq!(
            resolve_config_path(&e),
            Some(PathBuf::from("/tmp/xdg").join("herdr").join("config.toml"))
        );
    }

    #[test]
    fn unix_defaults_to_dot_config() {
        let e = PathEnv {
            home: Some(PathBuf::from("/home/me")),
            ..env()
        };
        assert_eq!(
            resolve_config_path(&e),
            Some(
                PathBuf::from("/home/me/.config")
                    .join("herdr")
                    .join("config.toml")
            )
        );
    }

    #[test]
    fn the_herdr_override_wins_on_every_platform() {
        for windows in [true, false] {
            let e = PathEnv {
                windows,
                override_path: Some(PathBuf::from("/somewhere/else.toml")),
                appdata: Some(PathBuf::from("C:/ignored")),
                xdg_config_home: Some(PathBuf::from("/ignored")),
                home: Some(PathBuf::from("/home/me")),
            };
            assert_eq!(
                resolve_config_path(&e),
                Some(PathBuf::from("/somewhere/else.toml"))
            );
        }
    }

    #[test]
    fn nothing_is_guessable_without_a_home() {
        assert_eq!(resolve_config_path(&env()), None);
        assert_eq!(
            resolve_config_path(&PathEnv {
                windows: true,
                ..env()
            }),
            None
        );
    }
}

#[cfg(test)]
mod newline_tests {
    use super::*;

    const CRLF: &str = "onboarding = false\r\n\r\n[theme]\r\nname = \"terminal\"\r\n";
    const LF: &str = "onboarding = false\n\n[theme]\nname = \"terminal\"\n";

    fn edit(path: &str, value: &str) -> Edit {
        Edit {
            path: path.into(),
            value: Some(value.to_string()),
            op: Op::Set,
        }
    }

    #[test]
    fn detects_the_style_in_use() {
        assert_eq!(Newline::detect(CRLF), Newline::Crlf);
        assert_eq!(Newline::detect(LF), Newline::Lf);
        assert_eq!(Newline::detect(""), Newline::Lf, "a new file gets LF");
    }

    #[test]
    fn a_crlf_file_stays_crlf() {
        let (after, _) = apply_edits(CRLF, &[edit("theme.name", "\"nord\"")]).unwrap();
        assert_eq!(
            after.matches("\r\n").count(),
            4,
            "every line ending survives: {after:?}"
        );
        assert_eq!(
            after.matches('\n').count(),
            after.matches("\r\n").count(),
            "no bare LF left behind"
        );
        assert_eq!(after, CRLF.replace("\"terminal\"", "\"nord\""));
    }

    #[test]
    fn an_lf_file_stays_lf() {
        let (after, _) = apply_edits(LF, &[edit("theme.name", "\"nord\"")]).unwrap();
        assert!(
            !after.contains('\r'),
            "CR must not be introduced: {after:?}"
        );
        assert_eq!(after, LF.replace("\"terminal\"", "\"nord\""));
    }

    #[test]
    fn a_table_added_to_a_crlf_file_uses_crlf() {
        let (after, _) = apply_edits(CRLF, &[edit("ui.sidebar_width", "30")]).unwrap();
        assert!(
            after.contains("[ui]\r\n"),
            "new header needs CRLF: {after:?}"
        );
        assert!(after.contains("sidebar_width = 30\r\n"));
        assert!(!after.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn removal_from_a_crlf_file_keeps_the_style() {
        let (after, _) = apply_edits(
            CRLF,
            &[Edit {
                path: "theme.name".into(),
                value: None,
                op: Op::Set,
            }],
        )
        .unwrap();
        assert!(!after.contains("name ="));
        assert!(!after.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn a_mixed_file_is_normalized_rather_than_left_ragged() {
        let mixed = "a = 1\r\nb = 2\n";
        let (after, _) = apply_edits(mixed, &[edit("a", "2")]).unwrap();
        assert_eq!(after, "a = 2\r\nb = 2\r\n");
    }
}

/// Exercises the real write path (backup, format-preserving write, `herdr
/// config check`) against a throwaway file via `HERDR_CONFIG_PATH`.
///
/// That is herdr's own override rather than one of ours, so the `herdr config
/// check` this triggers validates the very file we wrote.
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
        std::env::set_var("HERDR_CONFIG_PATH", &file);

        let result = save(
            vec![
                Edit {
                    path: "theme.name".into(),
                    value: Some("\"kanagawa\"".into()),
                    op: Op::Set,
                },
                Edit {
                    path: "theme.auto_switch".into(),
                    value: None,
                    op: Op::Set,
                },
                Edit {
                    path: "ui.sidebar_width".into(),
                    value: Some("30".into()),
                    op: Op::Set,
                },
            ],
            false,
        )
        .expect("save");

        let after = std::fs::read_to_string(&file).unwrap();
        assert!(after.contains("name = \"kanagawa\""));
        assert!(
            !after.contains("auto_switch"),
            "removed key still present:\n{after}"
        );
        assert!(
            after.contains("[ui]") && after.contains("sidebar_width = 30"),
            "{after}"
        );
        assert!(
            after.starts_with("onboarding = false\n"),
            "untouched head changed:\n{after}"
        );

        let actions: Vec<&str> = result.changes.iter().map(|c| c.action).collect();
        assert_eq!(actions, ["update", "remove", "add"]);

        let backup = result.backup.expect("backup path");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert!(
            result.reloaded.is_none(),
            "reload must not run when not requested"
        );

        std::env::remove_var("HERDR_CONFIG_PATH");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A value herdr cannot parse makes it discard the entire config, so such
    /// content must never reach disk. Skipped when herdr is unavailable,
    /// because the refusal comes from herdr's own verdict.
    #[test]
    fn a_config_herdr_would_reject_is_never_written() {
        let dir = std::env::temp_dir().join(format!("herdr-gui-reject-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        let original = "[theme]\nname = \"terminal\"\n";
        std::fs::write(&file, original).unwrap();
        std::env::set_var("HERDR_CONFIG_PATH", &file);

        // sidebar_width is a u16; a string breaks the whole file.
        let result = save(
            vec![Edit {
                path: "ui.sidebar_width".into(),
                value: Some("\"wide\"".into()),
                op: Op::Set,
            }],
            false,
        )
        .expect("save must report, not error out");

        if result.check.unavailable.is_some() {
            eprintln!("herdr not installed; skipping");
            std::env::remove_var("HERDR_CONFIG_PATH");
            std::fs::remove_dir_all(&dir).ok();
            return;
        }

        assert!(!result.written, "fatal content must not be written");
        assert!(result.check.fatal());
        assert!(
            result.check.discards_config,
            "herdr would fall back to defaults"
        );
        assert_eq!(result.backup, None, "nothing was overwritten, so no backup");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            original,
            "the file on disk is untouched"
        );

        // An unknown key is only ignored by herdr, so it is written.
        let ok = save(
            vec![Edit {
                path: "theme.custom.not_a_real_token".into(),
                value: Some("\"#ff0000\"".into()),
                op: Op::Set,
            }],
            false,
        )
        .expect("save");
        assert!(ok.written, "a warning must not block the save");
        assert!(!ok.check.fatal());
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("not_a_real_token"));

        std::env::remove_var("HERDR_CONFIG_PATH");
        std::fs::remove_dir_all(&dir).ok();
    }
}
