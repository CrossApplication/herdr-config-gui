//! herdr keybinding syntax, from a Slint key event.
//!
//! The shipping app builds this from a browser `KeyboardEvent`, which offers
//! both a logical key (`ev.key`) and a physical one (`ev.code`). Slint offers
//! only the logical key: its own documentation says bindings are "based on
//! logical keys -- the character a keypress produces on the current keyboard
//! layout -- not the physical position of a key". Everything below therefore
//! works from the character, which is what the terminal sends anyway; the one
//! case it cannot cover is noted on `from_event`.

/// Emission order. herdr's docs are inconsistent (`ctrl+shift+alt+left` but
/// also `alt+shift+left`), so we pick one and compare order-insensitively.
const MOD_ORDER: [&str; 4] = ["ctrl", "shift", "alt", "cmd"];

/// Punctuation herdr gives a name to.
const PUNCT: &[(char, &str)] = &[
    ('-', "minus"),
    (',', "comma"),
    ('&', "ampersand"),
    ('+', "plus"),
    ('`', "backtick"),
];

/// The indexed form, e.g. `switch_tab = "prefix+1..9"`.
pub const RANGE: &str = "1..9";

const MOD_ALIASES: &[(&str, &str)] = &[
    ("ctrl", "ctrl"),
    ("control", "ctrl"),
    ("shift", "shift"),
    ("alt", "alt"),
    ("option", "alt"),
    ("opt", "alt"),
    ("cmd", "cmd"),
    ("command", "cmd"),
    ("super", "cmd"),
    ("win", "cmd"),
    ("meta", "cmd"),
];

const KEY_ALIASES: &[(&str, &str)] = &[
    ("escape", "esc"),
    ("return", "enter"),
    ("arrowleft", "left"),
    ("arrowright", "right"),
    ("arrowup", "up"),
    ("arrowdown", "down"),
    ("spacebar", "space"),
];

const NAMED: &[&str] = &[
    "enter", "tab", "esc", "left", "right", "up", "down", "space",
];

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Chord {
    pub prefix: bool,
    pub mods: Vec<String>,
    pub key: String,
    /// True for the `1..9` indexed form.
    pub range: bool,
}

fn normalize_key(raw: &str) -> String {
    if raw == " " {
        return "space".into();
    }
    let k = raw.to_lowercase();
    if k == RANGE {
        return k;
    }
    if let Some((_, v)) = KEY_ALIASES.iter().find(|(a, _)| *a == k) {
        return (*v).to_string();
    }
    if let Some((_, name)) = PUNCT.iter().find(|(p, _)| p.to_string() == k) {
        return (*name).to_string();
    }
    k
}

/// Parse a herdr binding string. None for an empty or malformed one.
pub fn parse(text: &str) -> Option<Chord> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    // A trailing `+` can only be the plus key; herdr names it `plus`.
    let raw = match t.strip_suffix('+') {
        Some(head) => format!("{head}+plus"),
        None => t.to_string(),
    };

    let mut prefix = false;
    let mut mods: Vec<String> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for token in raw.split('+').filter(|x| !x.is_empty()) {
        let lower = token.to_lowercase();
        if lower == "prefix" {
            prefix = true;
        } else if let Some((_, m)) = MOD_ALIASES.iter().find(|(a, _)| *a == lower) {
            mods.push((*m).to_string());
        } else {
            keys.push(normalize_key(token));
        }
    }

    // Modifier-only values are how `[keys.indexed]` drives 1..9.
    if keys.is_empty() {
        return (prefix || !mods.is_empty()).then(|| Chord {
            prefix,
            mods: sort_mods(mods),
            key: RANGE.into(),
            range: true,
        });
    }
    if keys.len() > 1 {
        return None; // two non-modifier tokens is not a chord
    }
    let key = keys.remove(0);
    let range = key == RANGE;
    Some(Chord {
        prefix,
        mods: sort_mods(mods),
        key,
        range,
    })
}

fn sort_mods(mut mods: Vec<String>) -> Vec<String> {
    mods.sort_by_key(|m| MOD_ORDER.iter().position(|x| x == m).unwrap_or(9));
    mods.dedup();
    mods
}

impl Chord {
    /// Chords this occupies; a range covers nine of them.
    pub fn expand(&self) -> Vec<String> {
        if !self.range {
            return vec![self.canonical()];
        }
        (1..=9)
            .map(|n| {
                Chord {
                    key: n.to_string(),
                    range: false,
                    ..self.clone()
                }
                .canonical()
            })
            .collect()
    }

    pub fn canonical(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.prefix {
            parts.push("prefix".into());
        }
        parts.extend(self.mods.iter().cloned());
        parts.push(self.key.clone());
        parts.join("+")
    }
}

/// What the Slint layer reports for one keypress, plus the physical key
/// recovered from winit.
pub struct RawKey {
    /// Either a named key (`esc`, `left`, `f12`) resolved in the .slint file,
    /// or the character the layout produced. Slint's own contribution, and
    /// the equivalent of the browser's `ev.key`.
    pub key: String,
    /// The key's position, independent of layout and modifiers: the
    /// equivalent of the browser's `ev.code`. Only letters, digits and
    /// function keys are reported; anything else is better identified by the
    /// character.
    pub physical: Option<String>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// True for a key that only exists to modify another.
pub fn is_modifier_only(key: &str) -> bool {
    matches!(key, "shift" | "ctrl" | "alt" | "cmd" | "caps" | "")
}

/// Turn a keypress into herdr syntax, without any `prefix+`.
///
/// The two sources are used for what each is good at, the same split the
/// browser build makes:
///
///   - Letters, digits and function keys come from the physical key, because
///     shift uppercases the character and macOS composes it under alt --
///     `option+a` arrives as `a-ring`, and only the position says `a`.
///   - Symbols come from the character, because the terminal sends the
///     character itself: `shift+7` arrives as `&`, which herdr names
///     `ampersand`, and reporting shift alongside it would describe a chord
///     no terminal sends.
pub fn from_event(raw: &RawKey, mac: bool) -> Option<String> {
    if is_modifier_only(&raw.key) {
        return None;
    }
    let mut mods: Vec<String> = Vec::new();
    if raw.ctrl {
        mods.push("ctrl".into());
    }
    if raw.alt {
        mods.push("alt".into());
    }
    if raw.meta {
        mods.push(if mac { "cmd" } else { "super" }.into());
    }

    let chars: Vec<char> = raw.key.chars().collect();
    let printable = (chars.len() == 1).then(|| chars[0]);
    // A symbol the layout produced. `alt` is excluded because macOS composes
    // characters with it, which is not a symbol key.
    let symbol = printable.is_some_and(|c| !c.is_ascii_alphanumeric()) && !raw.alt;

    let (key, shift_inherent) = if symbol {
        let c = printable.unwrap();
        match PUNCT.iter().find(|(p, _)| *p == c) {
            Some((_, name)) => ((*name).to_string(), true),
            None => (c.to_string(), true),
        }
    } else if let Some(p) = raw.physical.as_deref() {
        (p.to_string(), false)
    } else if let Some(c) = printable {
        (c.to_ascii_lowercase().to_string(), false)
    } else {
        (raw.key.to_lowercase(), false)
    };

    if raw.shift && !shift_inherent {
        mods.push("shift".into());
    }

    Some(
        Chord {
            prefix: false,
            mods: sort_mods(mods),
            key,
            range: false,
        }
        .canonical(),
    )
}

pub fn with_prefix(chord: &str) -> String {
    if chord.starts_with("prefix+") {
        chord.to_string()
    } else {
        format!("prefix+{chord}")
    }
}

// --- validation ------------------------------------------------------------

fn is_fn_key(k: &str) -> bool {
    k.len() > 1 && k.starts_with('f') && k[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n))
}

fn is_named_punct(k: &str) -> bool {
    PUNCT.iter().any(|(_, name)| *name == k)
}

/// Rules herdr documents as prohibited. An empty list means acceptable.
pub fn validate(text: &str, kind: &str, accepts_range: bool) -> Vec<String> {
    let t = text.trim();
    if t.is_empty() {
        return Vec::new(); // unset / explicitly disabled
    }
    let Some(p) = parse(t) else {
        return vec![format!("\"{t}\" はキー構文として解釈できません")];
    };

    let mut errors = Vec::new();
    if p.range && !accepts_range {
        errors.push("この項目は 1..9 のレンジ表記を受け付けません".to_string());
    }
    let digit = p.key.len() == 1 && p.key.chars().next().is_some_and(|c| c.is_ascii_digit());

    match kind {
        "prefix" => {
            if p.prefix {
                errors.push("prefix キー自体に prefix+ は付けられません".into());
            }
            if p.range {
                errors.push("prefix キーにレンジ表記は使えません".into());
            }
        }
        "navigate" => {
            // Consumed by navigate mode's own modal loop.
            if p.prefix {
                errors.push("navigate モードのキーに prefix+ は使えません".into());
            }
            if ["esc", "enter", "tab"].contains(&p.key.as_str()) {
                errors.push(format!("navigate モードでは {} は予約されています", p.key));
            }
            if ["left", "right"].contains(&p.key.as_str()) {
                errors.push(format!(
                    "{} 矢印は常に左右のペイン移動に割り当てられています",
                    p.key
                ));
            }
            if digit && p.mods.is_empty() {
                errors.push("navigate モードでは修飾なしの 1〜9 は予約されています".into());
            }
        }
        "indexed" => {
            if p.prefix {
                errors.push("[keys.indexed] は修飾キーのみを指定します（prefix+ は不可）".into());
            }
            if !p.range {
                errors.push("[keys.indexed] は \"ctrl\" のように修飾キーのみを指定します".into());
            }
        }
        _ => {
            if !p.prefix
                && p.mods.is_empty()
                && !is_fn_key(&p.key)
                && !NAMED.contains(&p.key.as_str())
            {
                errors.push(
                    "修飾キーなしの直接バインドは通常の入力を奪うため使えません。prefix+ を付けてください"
                        .into(),
                );
            }
        }
    }
    errors
}

// --- terminal reliability --------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Risk {
    /// `safe` | `caution` | `risky`
    pub level: &'static str,
    pub reason: &'static str,
}

/// How likely the outer terminal is to deliver this chord. Prefix-mode
/// bindings are read by herdr itself once prefix mode is open, so they are
/// reliable whatever the chord.
pub fn risk(text: &str, kind: &str) -> Risk {
    let mk = |level, reason| Risk { level, reason };
    let t = text.trim();
    if t.is_empty() {
        return mk("safe", "未設定");
    }
    let Some(p) = parse(t) else {
        return mk("risky", "解釈できない構文");
    };

    if kind == "navigate" {
        return mk("safe", "navigate モード中のみ有効なローカルキー");
    }
    if kind == "indexed" {
        return if p.mods.iter().any(|m| m == "alt" || m == "cmd") {
            mk("risky", "alt / cmd は端末が横取りすることがあります")
        } else {
            mk("safe", "修飾キー + 1〜9")
        };
    }
    if p.prefix {
        return mk("safe", "prefix モード中に herdr が直接読み取ります");
    }
    if p.mods.iter().any(|m| m == "cmd") {
        return mk("risky", "cmd / super は端末やOSに奪われることが多い");
    }
    if p.mods.iter().any(|m| m == "alt") {
        return mk("risky", "alt は端末や tmux の設定次第で届きません");
    }
    if is_fn_key(&p.key) {
        return mk("safe", "ファンクションキーは直接バインドでも安定");
    }
    let letter = p.key.len() == 1 && p.key.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    if p.mods.len() == 1 && p.mods[0] == "ctrl" && letter {
        return mk("safe", "ctrl + 英字は直接バインドで最も安定");
    }
    let digit = p.key.len() == 1 && p.key.chars().next().is_some_and(|c| c.is_ascii_digit());
    if is_named_punct(&p.key) || (!letter && !digit && !NAMED.contains(&p.key.as_str())) {
        return mk("risky", "修飾キー付きの記号は端末依存です");
    }
    if p.mods.is_empty() {
        return mk("risky", "修飾キーなしの直接バインドは通常の入力を奪います");
    }
    mk(
        "caution",
        "明示的な修飾チョード。端末で実際に届くか確認してください",
    )
}

// --- conflicts -------------------------------------------------------------

/// Navigate-mode keys live in their own modal scope.
pub fn scope_of(kind: &str) -> &'static str {
    if kind == "navigate" {
        "navigate"
    } else {
        "global"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: String,
    pub value: String,
    pub kind: String,
    pub accepts_range: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    pub chord: String,
    pub scope: &'static str,
    pub paths: Vec<String>,
}

/// Bindings occupying the same chord in the same scope. Ranges are expanded,
/// so `prefix+1..9` collides with a hand-written `prefix+3`.
pub fn conflicts(entries: &[Entry]) -> Vec<Conflict> {
    let mut order: Vec<String> = Vec::new();
    let mut seen: std::collections::HashMap<String, Conflict> = Default::default();

    for e in entries {
        let Some(p) = parse(&e.value) else { continue };
        let scope = scope_of(&e.kind);
        for chord in p.expand() {
            let id = format!("{scope}::{chord}");
            match seen.get_mut(&id) {
                Some(hit) => hit.paths.push(e.path.clone()),
                None => {
                    order.push(id.clone());
                    seen.insert(
                        id,
                        Conflict {
                            chord,
                            scope,
                            paths: vec![e.path.clone()],
                        },
                    );
                }
            }
        }
    }

    let mut out: Vec<Conflict> = order
        .into_iter()
        .filter_map(|id| seen.remove(&id))
        .filter(|c| c.paths.len() > 1)
        .collect();
    out.sort_by(|a, b| a.chord.cmp(&b.chord));
    out
}

// --- problem report --------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    /// `invalid` | `conflict` | `risky`
    pub kind: &'static str,
    /// `error` | `warn`
    pub severity: &'static str,
    /// The chord or raw value at issue.
    pub chord: String,
    pub paths: Vec<String>,
    pub detail: String,
    pub scope: Option<&'static str>,
}

/// Everything wrong with the effective keybindings, worst first: values herdr
/// would reject, then chords claimed twice, then chords the outer terminal may
/// never deliver.
pub fn problems(bindings: &[Entry]) -> Vec<Problem> {
    let mut out = Vec::new();

    for b in bindings {
        for detail in validate(&b.value, &b.kind, b.accepts_range) {
            out.push(Problem {
                kind: "invalid",
                severity: "error",
                chord: b.value.clone(),
                paths: vec![b.path.clone()],
                detail,
                scope: None,
            });
        }
    }

    for c in conflicts(bindings) {
        out.push(Problem {
            kind: "conflict",
            severity: "error",
            chord: c.chord,
            detail: format!("{} 個の設定が同じキーに割り当たっています", c.paths.len()),
            paths: c.paths,
            scope: Some(c.scope),
        });
    }

    for b in bindings {
        if b.value.trim().is_empty() {
            continue;
        }
        let r = risk(&b.value, &b.kind);
        if r.level == "risky" {
            out.push(Problem {
                kind: "risky",
                severity: "warn",
                chord: b.value.clone(),
                paths: vec![b.path.clone()],
                detail: r.reason.to_string(),
                scope: None,
            });
        }
    }

    out
}

pub fn error_count(ps: &[Problem]) -> usize {
    ps.iter().filter(|p| p.severity == "error").count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(key: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> RawKey {
        RawKey {
            key: key.into(),
            physical: None,
            ctrl,
            shift,
            alt,
            meta,
        }
    }

    /// The same press, with the physical key winit supplies.
    fn raw_phys(
        key: &str,
        physical: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    ) -> RawKey {
        RawKey {
            key: key.into(),
            physical: Some(physical.into()),
            ctrl,
            shift,
            alt,
            meta,
        }
    }

    #[test]
    fn a_plain_chord_is_captured() {
        assert_eq!(
            from_event(&raw("b", true, false, false, false), true).unwrap(),
            "ctrl+b"
        );
    }

    #[test]
    fn shift_does_not_leak_into_the_letter() {
        // Slint reports the uppercase character; herdr wants letter + shift.
        assert_eq!(
            from_event(&raw("R", true, true, false, false), true).unwrap(),
            "ctrl+shift+r"
        );
    }

    #[test]
    fn a_symbol_already_carries_its_shift() {
        // shift+7 sends `&`, which herdr names `ampersand`. Reporting shift as
        // well would describe a chord no terminal sends.
        assert_eq!(
            from_event(&raw("&", false, true, false, false), true).unwrap(),
            "ampersand"
        );
        assert_eq!(
            from_event(&raw("-", false, false, false, false), true).unwrap(),
            "minus"
        );
        assert_eq!(
            from_event(&raw("|", false, true, false, false), true).unwrap(),
            "|"
        );
    }

    #[test]
    fn named_keys_pass_through_lowercased() {
        for (input, want) in [
            ("esc", "esc"),
            ("enter", "enter"),
            ("left", "left"),
            ("f12", "f12"),
        ] {
            assert_eq!(
                from_event(&raw(input, false, false, false, false), true).unwrap(),
                want
            );
        }
    }

    #[test]
    fn meta_is_cmd_on_macos_and_super_elsewhere() {
        assert_eq!(
            from_event(&raw("k", false, false, false, true), true).unwrap(),
            "cmd+k"
        );
        assert_eq!(
            from_event(&raw("k", false, false, false, true), false).unwrap(),
            "super+k"
        );
    }

    #[test]
    fn a_modifier_alone_keeps_the_capture_waiting() {
        for m in ["shift", "ctrl", "alt", "cmd", ""] {
            assert!(
                from_event(&raw(m, false, false, false, false), true).is_none(),
                "{m}"
            );
        }
    }

    #[test]
    fn modifier_order_is_canonical() {
        assert_eq!(
            from_event(&raw("left", true, true, true, false), true).unwrap(),
            "ctrl+shift+alt+left"
        );
    }

    #[test]
    fn the_physical_key_undoes_macos_alt_composition() {
        // Slint alone reports `a-ring` for option+a and cannot get back to
        // `a`. winit's physical key can, which is what the browser build does
        // with ev.code.
        assert_eq!(
            from_event(&raw("å", false, false, true, false), true).unwrap(),
            "alt+å"
        );
        assert_eq!(
            from_event(&raw_phys("å", "a", false, false, true, false), true).unwrap(),
            "alt+a"
        );
    }

    #[test]
    fn the_physical_key_is_preferred_for_letters_digits_and_function_keys() {
        // Shift uppercases the character, so the position is what identifies
        // the key.
        assert_eq!(
            from_event(&raw_phys("R", "r", true, true, false, false), true).unwrap(),
            "ctrl+shift+r"
        );
        assert_eq!(
            from_event(&raw_phys("1", "1", true, false, false, false), true).unwrap(),
            "ctrl+1"
        );
        assert_eq!(
            from_event(&raw_phys("f12", "f12", false, false, false, false), true).unwrap(),
            "f12"
        );
    }

    #[test]
    fn symbols_still_come_from_the_character_not_the_position() {
        // shift+7 is `&` on a US layout and something else elsewhere; herdr
        // wants the character, which is what the terminal sends.
        assert_eq!(
            from_event(&raw_phys("&", "7", false, true, false, false), true).unwrap(),
            "ampersand"
        );
        assert_eq!(
            from_event(&raw_phys("|", "backslash", false, true, false, false), true).unwrap(),
            "|"
        );
    }

    #[test]
    fn prefix_is_attached_once() {
        assert_eq!(with_prefix("shift+r"), "prefix+shift+r");
        assert_eq!(with_prefix("prefix+shift+r"), "prefix+shift+r");
    }

    /// Recorded from real key presses on macOS, to pin what Slint reports
    /// rather than what its documentation implies.
    #[test]
    fn real_macos_key_presses_produce_the_right_chords() {
        // Slint reports ctrl as ctrl and cmd as cmd: they are not swapped.
        assert_eq!(
            from_event(&raw("a", true, false, false, false), true).unwrap(),
            "ctrl+a"
        );
        assert_eq!(
            from_event(&raw("a", false, false, false, true), true).unwrap(),
            "cmd+a"
        );
        // A modifier arrives as its own press first, and must not end capture.
        for m in ["ctrl", "cmd", "alt", "shift"] {
            assert!(
                from_event(&raw(m, false, false, false, false), true).is_none(),
                "{m}"
            );
        }
        // shift+7 on a US layout.
        assert_eq!(
            from_event(&raw("&", false, true, false, false), true).unwrap(),
            "ampersand"
        );
        // cmd+shift+r, which is what gets sent when reaching for ctrl+shift+r.
        assert_eq!(
            from_event(&raw("R", false, true, false, true), true).unwrap(),
            "shift+cmd+r"
        );
        // Function keys and escape resolve through Slint's Key constants.
        assert_eq!(
            from_event(&raw("f12", false, false, false, false), true).unwrap(),
            "f12"
        );
        assert_eq!(
            from_event(&raw("esc", false, false, false, false), true).unwrap(),
            "esc"
        );
        // The one thing that cannot be recovered without a physical key.
        assert_eq!(
            from_event(&raw("å", false, false, true, false), true).unwrap(),
            "alt+å"
        );
    }

    #[test]
    fn terminal_risk_matches_the_shipping_rules() {
        assert_eq!(risk("prefix+alt+g", "action").level, "safe");
        assert_eq!(risk("ctrl+b", "action").level, "safe");
        assert_eq!(risk("f12", "action").level, "safe");
        assert_eq!(risk("cmd+k", "action").level, "risky");
        assert_eq!(risk("alt+shift+left", "action").level, "risky");
        assert_eq!(risk("n", "action").level, "risky");
        assert_eq!(risk("j", "navigate").level, "safe");
        assert_eq!(risk("ctrl+shift+r", "action").level, "caution");
    }
}

#[cfg(test)]
mod syntax_tests {
    use super::*;

    fn entry(path: &str, value: &str, kind: &str) -> Entry {
        Entry {
            path: path.into(),
            value: value.into(),
            kind: kind.into(),
            accepts_range: false,
        }
    }
    fn ranged(path: &str, value: &str) -> Entry {
        Entry {
            accepts_range: true,
            ..entry(path, value, "action")
        }
    }

    // --- parsing -----------------------------------------------------------

    #[test]
    fn modifier_order_does_not_change_identity() {
        // herdr's own docs write both `ctrl+shift+alt+left` and `alt+shift+left`.
        let a = parse("alt+shift+left").unwrap().canonical();
        let b = parse("shift+alt+left").unwrap().canonical();
        assert_eq!(a, b);
        assert_eq!(a, "shift+alt+left");
    }

    #[test]
    fn modifier_aliases_collapse() {
        for alias in ["control+a", "ctrl+a"] {
            assert_eq!(parse(alias).unwrap().canonical(), "ctrl+a", "{alias}");
        }
        for alias in ["cmd+a", "command+a", "super+a", "meta+a", "win+a"] {
            assert_eq!(parse(alias).unwrap().canonical(), "cmd+a", "{alias}");
        }
        for alias in ["alt+a", "option+a", "opt+a"] {
            assert_eq!(parse(alias).unwrap().canonical(), "alt+a", "{alias}");
        }
    }

    #[test]
    fn the_prefix_token_is_tracked_separately_from_modifiers() {
        let p = parse("prefix+shift+r").unwrap();
        assert!(p.prefix);
        assert_eq!(p.mods, ["shift"]);
        assert_eq!(p.key, "r");
    }

    #[test]
    fn a_trailing_plus_is_the_plus_key() {
        assert_eq!(parse("prefix++").unwrap().canonical(), "prefix+plus");
        assert_eq!(parse("prefix+plus").unwrap().canonical(), "prefix+plus");
    }

    #[test]
    fn a_modifier_only_value_is_the_range() {
        let p = parse("ctrl").unwrap();
        assert!(p.range);
        assert_eq!(p.key, RANGE);
    }

    #[test]
    fn empty_and_malformed_values_do_not_parse() {
        for bad in ["", "   ", "a+b"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_range_occupies_nine_chords() {
        assert_eq!(
            parse("prefix+1..9").unwrap().expand(),
            (1..=9).map(|n| format!("prefix+{n}")).collect::<Vec<_>>()
        );
        assert_eq!(parse("ctrl").unwrap().expand()[..2], ["ctrl+1", "ctrl+2"]);
    }

    // --- validation --------------------------------------------------------

    #[test]
    fn the_prefix_key_may_not_carry_prefix() {
        for ok in ["ctrl+a", "f12", "esc"] {
            assert!(validate(ok, "prefix", false).is_empty(), "{ok}");
        }
        assert!(validate("prefix+a", "prefix", false)[0].contains("prefix+ は付けられません"));
    }

    #[test]
    fn navigate_mode_rejects_the_keys_herdr_reserves() {
        for ok in ["j", "k", "up", "down", "h", "l"] {
            assert!(validate(ok, "navigate", false).is_empty(), "{ok}");
        }
        for bad in ["prefix+j", "esc", "enter", "tab", "left", "right", "3"] {
            assert!(!validate(bad, "navigate", false).is_empty(), "{bad}");
        }
    }

    #[test]
    fn indexed_bindings_take_modifiers_only() {
        assert!(validate("ctrl", "indexed", true).is_empty());
        assert!(validate("ctrl+shift", "indexed", true).is_empty());
        assert!(!validate("ctrl+t", "indexed", true).is_empty());
        assert!(!validate("prefix+ctrl", "indexed", true).is_empty());
    }

    #[test]
    fn an_unmodified_direct_binding_is_rejected_for_actions() {
        assert!(!validate("n", "action", false).is_empty());
        assert!(validate("prefix+n", "action", false).is_empty());
        assert!(validate("ctrl+alt+n", "action", false).is_empty());
        assert!(
            validate("f12", "action", false).is_empty(),
            "function keys need no modifier"
        );
    }

    #[test]
    fn the_range_form_is_only_allowed_where_the_schema_says_so() {
        assert!(validate("prefix+1..9", "action", true).is_empty());
        assert!(validate("prefix+1..9", "action", false)[0].contains("レンジ表記を受け付けません"));
    }

    #[test]
    fn an_empty_binding_is_always_acceptable() {
        for kind in ["prefix", "action", "navigate", "indexed", "command"] {
            assert!(validate("", kind, false).is_empty(), "{kind}");
        }
    }

    // --- conflicts ---------------------------------------------------------

    #[test]
    fn two_bindings_on_the_same_chord_conflict() {
        let c = conflicts(&[
            entry("keys.new_tab", "prefix+c", "action"),
            entry("keys.close_pane", "prefix+c", "action"),
        ]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].chord, "prefix+c");
        assert_eq!(c[0].paths, ["keys.new_tab", "keys.close_pane"]);
    }

    #[test]
    fn conflicts_ignore_modifier_order() {
        let c = conflicts(&[
            entry("a", "alt+shift+left", "action"),
            entry("b", "shift+alt+left", "action"),
        ]);
        assert_eq!(c.len(), 1, "the same chord written two ways must collide");
    }

    #[test]
    fn a_range_collides_with_a_single_binding_inside_it() {
        let c = conflicts(&[
            ranged("keys.switch_tab", "prefix+1..9"),
            entry("keys.goto", "prefix+3", "action"),
        ]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].chord, "prefix+3");
    }

    #[test]
    fn navigate_keys_do_not_collide_with_global_bindings() {
        // herdr: navigate-mode shortcuts are independent from focus_pane_*.
        let c = conflicts(&[
            entry("keys.navigate_pane_down", "j", "navigate"),
            entry("keys.focus_pane_down", "j", "action"),
        ]);
        assert!(c.is_empty(), "different scopes");
        assert_eq!(scope_of("navigate"), "navigate");
        assert_eq!(scope_of("action"), "global");
    }

    #[test]
    fn navigate_keys_still_collide_with_each_other() {
        let c = conflicts(&[
            entry("keys.navigate_pane_down", "j", "navigate"),
            entry("keys.navigate_workspace_down", "j", "navigate"),
        ]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].scope, "navigate");
    }

    #[test]
    fn unset_bindings_never_conflict() {
        assert!(conflicts(&[
            entry("a", "", "action"),
            entry("b", "", "action"),
            entry("c", "   ", "action"),
        ])
        .is_empty());
    }

    #[test]
    fn the_prefix_key_occupies_its_chord_globally() {
        let c = conflicts(&[
            entry("keys.prefix", "ctrl+a", "prefix"),
            entry("keys.remote_image_paste", "ctrl+a", "action"),
        ]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].chord, "ctrl+a");
    }

    // --- problems ----------------------------------------------------------

    #[test]
    fn problems_are_ordered_worst_first() {
        let ps = problems(&[
            entry("keys.remote_image_paste", "cmd+v", "action"),
            entry("keys.new_tab", "prefix+c", "action"),
            entry("keys.close_pane", "prefix+c", "action"),
            entry("keys.navigate_pane_up", "esc", "navigate"),
        ]);
        assert_eq!(
            ps.iter().map(|p| p.kind).collect::<Vec<_>>(),
            ["invalid", "conflict", "risky"]
        );
        assert_eq!(error_count(&ps), 2, "risky is a warning, not an error");
    }

    #[test]
    fn a_conflict_names_every_setting_involved() {
        let ps = problems(&[
            entry("keys.workspace_picker", "prefix+w", "action"),
            entry("keys.next_workspace", "prefix+w", "action"),
        ]);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].kind, "conflict");
        assert_eq!(
            ps[0].paths,
            ["keys.workspace_picker", "keys.next_workspace"]
        );
        assert_eq!(ps[0].scope, Some("global"));
    }

    #[test]
    fn clearing_one_side_of_a_conflict_resolves_it() {
        let after = problems(&[
            entry("keys.workspace_picker", "prefix+w", "action"),
            entry("keys.next_workspace", "", "action"),
        ]);
        assert!(after.is_empty());
    }

    /// Every binding herdr ships must pass, or the rules are too strict.
    #[test]
    fn the_bindings_herdr_ships_raise_no_problems() {
        let schema = crate::schema::build(include_str!("../fixtures/default-config.toml"));
        let bindings: Vec<Entry> = schema
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .filter_map(|i| {
                i.binding_kind.map(|kind| Entry {
                    path: i.path.clone(),
                    // The schema stores verbatim TOML; strip the quotes.
                    value: i.default.trim_matches('"').to_string(),
                    kind: kind.to_string(),
                    accepts_range: i.accepts_range,
                })
            })
            .collect();
        assert!(bindings.len() >= 58, "got {}", bindings.len());
        assert_eq!(problems(&bindings), Vec::new());
    }
}
