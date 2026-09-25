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

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Chord {
    pub prefix: bool,
    pub mods: Vec<String>,
    pub key: String,
}

fn sort_mods(mut mods: Vec<String>) -> Vec<String> {
    mods.sort_by_key(|m| MOD_ORDER.iter().position(|x| x == m).unwrap_or(9));
    mods.dedup();
    mods
}

impl Chord {
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

/// How likely the outer terminal is to deliver this chord.
pub fn risk(binding: &str, kind: &str) -> (&'static str, &'static str) {
    if binding.trim().is_empty() {
        return ("safe", "未設定");
    }
    if kind == "navigate" {
        return ("safe", "navigate モード中のみ有効なローカルキー");
    }
    if binding.starts_with("prefix+") {
        return ("safe", "prefix モード中に herdr が直接読み取ります");
    }
    if binding.contains("cmd+") || binding.contains("super+") {
        return ("risky", "cmd / super は端末やOSに奪われることが多い");
    }
    if binding.contains("alt+") {
        return ("risky", "alt は端末や tmux の設定次第で届きません");
    }
    let key = binding.rsplit('+').next().unwrap_or("");
    if key.starts_with('f') && key[1..].chars().all(|c| c.is_ascii_digit()) && key.len() > 1 {
        return ("safe", "ファンクションキーは直接バインドでも安定");
    }
    if binding.starts_with("ctrl+") && binding.len() == 6 {
        return ("safe", "ctrl + 英字は直接バインドで最も安定");
    }
    if !binding.contains('+') {
        return ("risky", "修飾キーなしの直接バインドは通常の入力を奪います");
    }
    ("caution", "明示的な修飾チョード。端末で実際に届くか確認してください")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(key: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> RawKey {
        RawKey { key: key.into(), physical: None, ctrl, shift, alt, meta }
    }

    /// The same press, with the physical key winit supplies.
    fn raw_phys(key: &str, physical: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> RawKey {
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
        assert_eq!(from_event(&raw("b", true, false, false, false), true).unwrap(), "ctrl+b");
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
        assert_eq!(from_event(&raw("&", false, true, false, false), true).unwrap(), "ampersand");
        assert_eq!(from_event(&raw("-", false, false, false, false), true).unwrap(), "minus");
        assert_eq!(from_event(&raw("|", false, true, false, false), true).unwrap(), "|");
    }

    #[test]
    fn named_keys_pass_through_lowercased() {
        for (input, want) in [("esc", "esc"), ("enter", "enter"), ("left", "left"), ("f12", "f12")] {
            assert_eq!(from_event(&raw(input, false, false, false, false), true).unwrap(), want);
        }
    }

    #[test]
    fn meta_is_cmd_on_macos_and_super_elsewhere() {
        assert_eq!(from_event(&raw("k", false, false, false, true), true).unwrap(), "cmd+k");
        assert_eq!(from_event(&raw("k", false, false, false, true), false).unwrap(), "super+k");
    }

    #[test]
    fn a_modifier_alone_keeps_the_capture_waiting() {
        for m in ["shift", "ctrl", "alt", "cmd", ""] {
            assert!(from_event(&raw(m, false, false, false, false), true).is_none(), "{m}");
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
        assert_eq!(from_event(&raw("å", false, false, true, false), true).unwrap(), "alt+å");
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
        assert_eq!(from_event(&raw_phys("1", "1", true, false, false, false), true).unwrap(), "ctrl+1");
        assert_eq!(from_event(&raw_phys("f12", "f12", false, false, false, false), true).unwrap(), "f12");
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
        assert_eq!(from_event(&raw("a", true, false, false, false), true).unwrap(), "ctrl+a");
        assert_eq!(from_event(&raw("a", false, false, false, true), true).unwrap(), "cmd+a");
        // A modifier arrives as its own press first, and must not end capture.
        for m in ["ctrl", "cmd", "alt", "shift"] {
            assert!(from_event(&raw(m, false, false, false, false), true).is_none(), "{m}");
        }
        // shift+7 on a US layout.
        assert_eq!(from_event(&raw("&", false, true, false, false), true).unwrap(), "ampersand");
        // cmd+shift+r, which is what gets sent when reaching for ctrl+shift+r.
        assert_eq!(
            from_event(&raw("R", false, true, false, true), true).unwrap(),
            "shift+cmd+r"
        );
        // Function keys and escape resolve through Slint's Key constants.
        assert_eq!(from_event(&raw("f12", false, false, false, false), true).unwrap(), "f12");
        assert_eq!(from_event(&raw("esc", false, false, false, false), true).unwrap(), "esc");
        // The one thing that cannot be recovered without a physical key.
        assert_eq!(from_event(&raw("å", false, false, true, false), true).unwrap(), "alt+å");
    }

    #[test]
    fn terminal_risk_matches_the_shipping_rules() {
        assert_eq!(risk("prefix+alt+g", "action").0, "safe");
        assert_eq!(risk("ctrl+b", "action").0, "safe");
        assert_eq!(risk("f12", "action").0, "safe");
        assert_eq!(risk("cmd+k", "action").0, "risky");
        assert_eq!(risk("alt+shift+left", "action").0, "risky");
        assert_eq!(risk("n", "action").0, "risky");
        assert_eq!(risk("j", "navigate").0, "safe");
        assert_eq!(risk("ctrl+shift+r", "action").0, "caution");
    }
}
