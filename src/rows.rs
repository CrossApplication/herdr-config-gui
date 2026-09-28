//! Sidebar rows, the Rust counterpart of the shipping app's `src/rows.ts`.
//!
//! Shorter than the TypeScript version because `toml_edit` already parses
//! TOML: the 90 lines of hand-written tokenizer there reduce to walking a
//! parsed value here.
//!
//! Parsing stays narrow on purpose. Anything that is not a shape the editor
//! can represent returns None, and the setting falls back to its raw TOML
//! field rather than being rewritten from a half-matched structure.

use toml_edit::Value;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TokenSpec {
    /// A built-in name, or `$name` for a metadata value.
    pub token: String,
    /// `#rgb` or `#rrggbb`; herdr rejects every other notation here.
    pub fg: Option<String>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
}

pub type Rows = Vec<Vec<TokenSpec>>;

fn token_from(value: &Value) -> Option<TokenSpec> {
    match value {
        Value::String(s) => Some(TokenSpec {
            token: s.value().clone(),
            ..Default::default()
        }),
        Value::InlineTable(t) => {
            let mut spec = TokenSpec {
                token: t.get("token")?.as_str()?.to_string(),
                ..Default::default()
            };
            for (key, v) in t.iter() {
                match key {
                    "token" => {}
                    "fg" => spec.fg = Some(v.as_str()?.to_string()),
                    "bold" => spec.bold = Some(v.as_bool()?),
                    "dim" => spec.dim = Some(v.as_bool()?),
                    // Not a field herdr accepts, so not something we can edit.
                    _ => return None,
                }
            }
            Some(spec)
        }
        _ => None,
    }
}

pub fn parse(text: &str) -> Option<Rows> {
    let value: Value = text.trim().parse().ok()?;
    let outer = value.as_array()?;
    let mut rows = Rows::new();
    for row in outer.iter() {
        let inner = row.as_array()?;
        rows.push(inner.iter().map(token_from).collect::<Option<Vec<_>>>()?);
    }
    Some(rows)
}

fn quote(s: &str) -> String {
    format!("{s:?}")
}

fn token_to_toml(t: &TokenSpec) -> String {
    if t.fg.is_none() && t.bold.is_none() && t.dim.is_none() {
        return quote(&t.token);
    }
    let mut parts = vec![format!("token = {}", quote(&t.token))];
    if let Some(fg) = &t.fg {
        parts.push(format!("fg = {}", quote(fg)));
    }
    if let Some(b) = t.bold {
        parts.push(format!("bold = {b}"));
    }
    if let Some(d) = t.dim {
        parts.push(format!("dim = {d}"));
    }
    format!("{{ {} }}", parts.join(", "))
}

pub fn to_toml(rows: &Rows) -> String {
    let body = rows
        .iter()
        .map(|row| {
            let cells = row.iter().map(token_to_toml).collect::<Vec<_>>().join(", ");
            format!("[{cells}]")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}

pub const CUSTOM_OPTION: &str = "$ カスタム値…";

/// Move a token within its row; at an edge it hops to the neighbouring row.
pub fn move_token(rows: &mut Rows, row: usize, index: usize, delta: isize) {
    let to = index as isize + delta;
    if to >= 0 && (to as usize) < rows[row].len() {
        let t = rows[row].remove(index);
        rows[row].insert(to as usize, t);
        return;
    }
    let target = row as isize + delta;
    if target < 0 || target as usize >= rows.len() {
        return;
    }
    let t = rows[row].remove(index);
    if delta < 0 {
        rows[target as usize].push(t);
    } else {
        rows[target as usize].insert(0, t);
    }
}

pub fn move_row(rows: &mut Rows, index: usize, delta: isize) {
    let to = index as isize + delta;
    if to < 0 || to as usize >= rows.len() {
        return;
    }
    let row = rows.remove(index);
    rows.insert(to as usize, row);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_defaults_round_trip() {
        for source in [
            "[]",
            r#"[["state_icon", "machine", "workspace", "tab"], ["agent"]]"#,
            r#"[["state_icon", "workspace"], ["branch", "git_status"]]"#,
            r##"[[{ token = "workspace", fg = "#89b4fa", bold = true, dim = false }]]"##,
            r#"[["state_icon", { token = "agent", dim = true }], ["$jj_status"]]"#,
        ] {
            let parsed = parse(source).unwrap_or_else(|| panic!("failed to parse {source}"));
            assert_eq!(to_toml(&parsed), source, "{source}");
        }
    }

    #[test]
    fn shapes_the_editor_cannot_represent_are_refused() {
        for bad in [
            r#"["state_icon"]"#,
            r#"[[{ token = "workspace", italic = true }]]"#,
            r##"[[{ fg = "#fff" }]]"##,
            "[[123]]",
            "not an array",
        ] {
            assert!(parse(bad).is_none(), "{bad} should not parse");
        }
    }

    #[test]
    fn a_token_at_the_edge_hops_rows() {
        let mut rows = parse(r#"[["a", "b"], ["c"]]"#).unwrap();
        move_token(&mut rows, 0, 1, 1);
        assert_eq!(to_toml(&rows), r#"[["a"], ["b", "c"]]"#);
    }

    #[test]
    fn rows_move_and_stop_at_the_ends() {
        let mut rows = parse(r#"[["a"], ["b"]]"#).unwrap();
        move_row(&mut rows, 1, -1);
        assert_eq!(to_toml(&rows), r#"[["b"], ["a"]]"#);
        move_row(&mut rows, 0, -1);
        assert_eq!(to_toml(&rows), r#"[["b"], ["a"]]"#);
    }
}
