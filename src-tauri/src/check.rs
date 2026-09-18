//! Validating a candidate config.toml by asking herdr.
//!
//! `herdr config check` is the only authority on what herdr accepts, and it
//! says more than pass/fail: it names unknown sections and keys, the type it
//! expected, and the members of an enum. Pointing `HERDR_CONFIG_PATH` at a
//! temporary file lets us run it on content that has not been written yet.
//!
//! The distinction that matters is `; using defaults`. An unknown key is
//! ignored and the rest of the file still applies, but a parse or type error
//! makes herdr discard the whole config, so saving one would silently revert
//! every setting the user has.

use serde::Serialize;
use std::path::PathBuf;

use crate::herdr;

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// herdr falls back to defaults for the entire file.
    Error,
    /// herdr ignores this one item and applies the rest.
    Warning,
}

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    UnknownSection,
    UnknownKey,
    Type,
    Variant,
    Syntax,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: Kind,
    /// Verbatim message from herdr.
    pub message: String,
    /// Config path or key name, when herdr named one.
    pub path: Option<String>,
    pub line: Option<usize>,
    /// The type herdr expected, e.g. `u16`.
    pub expected: Option<String>,
    /// Accepted values, when the message listed them.
    pub allowed: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CheckReport {
    /// herdr reported no issues at all.
    pub ok: bool,
    /// herdr would discard the whole config.
    pub discards_config: bool,
    pub diagnostics: Vec<Diagnostic>,
    /// Verbatim output, so nothing is hidden from the user.
    pub raw: String,
    /// Set when herdr could not be run at all.
    pub unavailable: Option<String>,
}

impl CheckReport {
    pub fn fatal(&self) -> bool {
        self.discards_config
            || self
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
    }
}

fn backticked(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else { break };
        let value = &after[..end];
        // An empty pair is the offending value being quoted, not a member.
        if !value.is_empty() && !out.iter().any(|v| v == value) {
            out.push(value.to_string());
        }
        rest = &after[end + 1..];
    }
    out
}

/// Pull the key name out of the snippet line herdr prints under a parse error:
/// `2 | sidebar_width = "not a number"`.
fn snippet_key(line: &str) -> Option<String> {
    let (_, after) = line.split_once('|')?;
    let name: String = after
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let after_name = after.trim_start()[name.len()..].trim_start();
    (!name.is_empty() && after_name.starts_with('=')).then_some(name)
}

pub fn parse_output(out: &str) -> CheckReport {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut discards_config = false;

    // State while walking a `config parse error:` block.
    let mut in_parse_error = false;
    let mut err_line: Option<usize> = None;
    let mut err_key: Option<String> = None;
    let mut detail: Option<Diagnostic> = None;

    let flush = |detail: &mut Option<Diagnostic>, diagnostics: &mut Vec<Diagnostic>| {
        if let Some(d) = detail.take() {
            diagnostics.push(d);
        }
    };

    for raw_line in out.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "config: ok" || line == "config: issues found" {
            continue;
        }
        if line.starts_with("__EXIT__=") {
            continue; // only present in captured fixtures
        }

        if let Some(rest) = line.strip_prefix("unknown config section ") {
            let name = rest.split(';').next().unwrap_or("").trim();
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                kind: Kind::UnknownSection,
                message: line.to_string(),
                path: Some(name.trim_matches(['[', ']']).to_string()),
                line: None,
                expected: None,
                allowed: Vec::new(),
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("unknown config key ") {
            let name = rest.split(';').next().unwrap_or("").trim();
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                kind: Kind::UnknownKey,
                message: line.to_string(),
                path: Some(name.to_string()),
                line: None,
                expected: None,
                allowed: Vec::new(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("config parse error:") {
            flush(&mut detail, &mut diagnostics);
            in_parse_error = true;
            err_key = None;
            err_line = rest
                .split("at line ")
                .nth(1)
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.trim().parse().ok());
            continue;
        }

        if line.starts_with("; using defaults") {
            discards_config = true;
            flush(&mut detail, &mut diagnostics);
            in_parse_error = false;
            continue;
        }

        if !in_parse_error {
            continue;
        }

        // Inside a parse-error block.
        if line == "|" || line.starts_with("^") {
            continue;
        }
        if let Some(key) = snippet_key(line) {
            err_key = Some(key);
            continue;
        }
        if line.split_once('|').is_some() {
            continue; // snippet line without an assignment, e.g. a table header
        }

        let (kind, expected, allowed) = if let Some(rest) = line.strip_prefix("invalid type:") {
            let expected = rest.split("expected ").nth(1).map(|s| s.trim().to_string());
            (Kind::Type, expected, Vec::new())
        } else if line.starts_with("unknown variant") {
            let after = line.split("expected").nth(1).unwrap_or("");
            (Kind::Variant, None, backticked(after))
        } else {
            (Kind::Syntax, None, Vec::new())
        };

        match &mut detail {
            // Continuation lines such as "expected `.`, `]`" after
            // "invalid table header".
            Some(d) if kind == Kind::Syntax => {
                d.message.push_str("; ");
                d.message.push_str(line);
                d.allowed.extend(backticked(line));
            }
            _ => {
                flush(&mut detail, &mut diagnostics);
                detail = Some(Diagnostic {
                    severity: Severity::Error,
                    kind,
                    message: line.to_string(),
                    path: err_key.clone(),
                    line: err_line,
                    expected,
                    allowed,
                });
            }
        }
    }
    flush(&mut detail, &mut diagnostics);

    CheckReport {
        ok: diagnostics.is_empty() && !discards_config,
        discards_config,
        diagnostics,
        raw: out.trim().to_string(),
        unavailable: None,
    }
}

fn temp_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "herdr-config-gui-check-{}-{stamp}.toml",
        std::process::id()
    ))
}

/// Ask herdr to validate content that has not been written anywhere yet.
pub fn check_toml(raw: &str) -> CheckReport {
    let path = temp_path();
    if let Err(e) = std::fs::write(&path, raw) {
        return CheckReport {
            ok: false,
            discards_config: false,
            diagnostics: Vec::new(),
            raw: String::new(),
            unavailable: Some(format!("could not write a temporary file: {e}")),
        };
    }
    let result = herdr::run_against_config(&["config", "check"], &path);
    std::fs::remove_file(&path).ok();

    match result {
        Ok(out) => parse_output(&out),
        Err(e) => CheckReport {
            ok: false,
            discards_config: false,
            diagnostics: Vec::new(),
            raw: String::new(),
            unavailable: Some(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verbatim `herdr config check` output, captured from herdr 0.9.0.
    const OK: &str = include_str!("../fixtures/check/ok.txt");
    const UNKNOWN: &str = include_str!("../fixtures/check/unknown.txt");
    const TYPE_ERROR: &str = include_str!("../fixtures/check/type-error.txt");
    const ENUM_ERROR: &str = include_str!("../fixtures/check/enum-error.txt");
    const ENUM_LIST: &str = include_str!("../fixtures/check/enum-list.txt");
    const SYNTAX_ERROR: &str = include_str!("../fixtures/check/syntax-error.txt");

    #[test]
    fn a_clean_config_reports_nothing() {
        let r = parse_output(OK);
        assert!(r.ok);
        assert!(!r.fatal());
        assert!(r.diagnostics.is_empty());
    }

    #[test]
    fn unknown_sections_and_keys_are_warnings() {
        let r = parse_output(UNKNOWN);
        assert!(!r.ok);
        // herdr ignores them and applies the rest, so saving is still safe.
        assert!(!r.fatal(), "unknown keys must not block a save");
        assert!(!r.discards_config);
        assert_eq!(r.diagnostics.len(), 2);

        let sec = &r.diagnostics[0];
        assert_eq!(sec.kind, Kind::UnknownSection);
        assert_eq!(sec.severity, Severity::Warning);
        assert_eq!(sec.path.as_deref(), Some("bogus_section"));

        let key = &r.diagnostics[1];
        assert_eq!(key.kind, Kind::UnknownKey);
        assert_eq!(
            key.path.as_deref(),
            Some("theme.custom.totally_bogus_token")
        );
    }

    #[test]
    fn a_type_error_discards_the_whole_config() {
        let r = parse_output(TYPE_ERROR);
        assert!(r.fatal(), "this must block a save");
        assert!(r.discards_config);
        assert_eq!(r.diagnostics.len(), 1);
        let d = &r.diagnostics[0];
        assert_eq!(d.kind, Kind::Type);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.expected.as_deref(), Some("u16"));
        assert_eq!(d.path.as_deref(), Some("sidebar_width"));
        assert_eq!(d.line, Some(2));
    }

    #[test]
    fn a_two_member_enum_yields_both_members() {
        let r = parse_output(ENUM_ERROR);
        assert!(r.fatal());
        let d = &r.diagnostics[0];
        assert_eq!(d.kind, Kind::Variant);
        assert_eq!(d.path.as_deref(), Some("agent_panel_scope"));
        // The offending value is an empty pair of backticks and must not be
        // mistaken for a member.
        assert_eq!(d.allowed, ["current", "all"]);
    }

    #[test]
    fn a_longer_enum_yields_every_member() {
        let r = parse_output(ENUM_LIST);
        let d = &r.diagnostics[0];
        assert_eq!(d.kind, Kind::Variant);
        assert_eq!(d.path.as_deref(), Some("claude"));
        // Only what comes after "expected": the rejected value `loud` is not
        // a member and must not be offered as one.
        assert_eq!(d.allowed, ["default", "on", "off"]);
    }

    #[test]
    fn a_syntax_error_is_reported_without_a_key() {
        let r = parse_output(SYNTAX_ERROR);
        assert!(r.fatal());
        assert_eq!(r.diagnostics.len(), 1, "{:?}", r.diagnostics);
        let d = &r.diagnostics[0];
        assert_eq!(d.kind, Kind::Syntax);
        assert_eq!(d.path, None);
        assert_eq!(d.line, Some(1));
        assert!(d.message.contains("invalid table header"));
    }

    /// End-to-end against the installed binary: the temporary file must be
    /// validated and then cleaned up. Skipped when herdr is unavailable.
    #[test]
    fn check_toml_runs_against_the_real_binary() {
        let clean = check_toml("[theme]\nname = \"terminal\"\n");
        if clean.unavailable.is_some() {
            eprintln!("herdr not installed; skipping");
            return;
        }
        assert!(clean.ok, "{clean:?}");

        let broken = check_toml("[ui]\nsidebar_width = \"nope\"\n");
        assert!(broken.fatal());
        assert_eq!(broken.diagnostics[0].expected.as_deref(), Some("u16"));

        let warned = check_toml("[theme.custom]\nnot_a_token = \"#fff\"\n");
        assert!(!warned.fatal(), "an unknown key is not fatal");
        assert_eq!(warned.diagnostics[0].kind, Kind::UnknownKey);

        // No temporary files left behind.
        let leaked: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("herdr-config-gui-check-")
            })
            .collect();
        assert!(leaked.is_empty(), "temp files left: {leaked:?}");
    }

    #[test]
    fn the_verbatim_output_is_always_kept() {
        for out in [OK, UNKNOWN, TYPE_ERROR, SYNTAX_ERROR] {
            assert!(!parse_output(out).raw.is_empty());
        }
    }
}
