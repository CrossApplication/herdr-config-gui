//! Colour handling for the picker.
//!
//! herdr accepts several notations depending on where the value sits:
//! `[theme.custom]` takes hex, named colours, `rgb(r,g,b)` and `reset`, while
//! a sidebar row's `fg` takes only `#rgb` / `#rrggbb`. The picker always
//! produces hex, which is valid everywhere, and the text field stays open so
//! the other notations can still be typed.
//!
//! herdr does not validate colours at all in `[theme.custom]` -- `accent =
//! "notacolor"` passes `config check` and is then silently ignored -- so this
//! is the only place a mistake gets caught.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Form {
    Empty,
    Reset,
    Hex,
    Rgb,
    Name,
    Malformed,
}

/// Names used only to preview a swatch; herdr may accept more than these.
const NAMED: &[(&str, (u8, u8, u8))] = &[
    ("black", (0, 0, 0)),
    ("red", (204, 0, 0)),
    ("green", (78, 154, 6)),
    ("yellow", (196, 160, 0)),
    ("blue", (52, 101, 164)),
    ("magenta", (117, 80, 123)),
    ("cyan", (6, 152, 154)),
    ("white", (211, 215, 207)),
    ("gray", (128, 128, 128)),
    ("grey", (128, 128, 128)),
];

fn parse_rgb_call(t: &str) -> Option<(u8, u8, u8)> {
    let inner = t.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    let mut out = [0u8; 3];
    for (i, p) in parts.iter().enumerate() {
        let n: u32 = p.parse().ok()?;
        if n > 255 {
            return None;
        }
        out[i] = n as u8;
    }
    Some((out[0], out[1], out[2]))
}

pub fn parse_hex(t: &str) -> Option<(u8, u8, u8)> {
    let hex = t.strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 => {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 1].repeat(2), 16).ok();
            Some((d(0)?, d(1)?, d(2)?))
        }
        6 => {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some((d(0)?, d(2)?, d(4)?))
        }
        _ => None,
    }
}

pub fn form(text: &str) -> Form {
    let t = text.trim();
    if t.is_empty() {
        return Form::Empty;
    }
    if t.eq_ignore_ascii_case("reset") {
        return Form::Reset;
    }
    if parse_hex(t).is_some() {
        return Form::Hex;
    }
    if t.starts_with('#') {
        return Form::Malformed;
    }
    if t.starts_with("rgb") {
        return if parse_rgb_call(t).is_some() {
            Form::Rgb
        } else {
            Form::Malformed
        };
    }
    // An unknown name may still be valid: herdr's list is not documented, so
    // warning about it would be worse than silence.
    if t.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Form::Name
    } else {
        Form::Malformed
    }
}

/// RGB for the swatch, or None when the value cannot be previewed.
pub fn to_rgb(text: &str) -> Option<(u8, u8, u8)> {
    let t = text.trim();
    parse_hex(t).or_else(|| parse_rgb_call(t)).or_else(|| {
        let lower = t.to_ascii_lowercase();
        NAMED.iter().find(|(n, _)| *n == lower).map(|(_, rgb)| *rgb)
    })
}

pub fn to_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// h in 0..1, s and v in 0..1.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (q(r), q(g), q(b))
}

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_notations_are_recognised() {
        // herdr: hex (#rrggbb), named colours, rgb(r,g,b), or "reset".
        assert_eq!(form("#f5c2e7"), Form::Hex);
        assert_eq!(form("#fff"), Form::Hex);
        assert_eq!(form("rgb(137, 180, 250)"), Form::Rgb);
        assert_eq!(form("cyan"), Form::Name);
        assert_eq!(form("reset"), Form::Reset);
        assert_eq!(form(""), Form::Empty);
    }

    #[test]
    fn a_broken_hex_or_rgb_is_called_out() {
        for bad in ["#12345", "#zzzzzz", "rgb(1,2)", "rgb(300,0,0)", "#"] {
            assert_eq!(form(bad), Form::Malformed, "{bad}");
        }
    }

    #[test]
    fn an_unknown_name_is_accepted_because_herdrs_list_is_undocumented() {
        assert_eq!(form("rosewater"), Form::Name);
        assert_eq!(form("subtext1"), Form::Name);
    }

    #[test]
    fn previews_expand_and_convert() {
        assert_eq!(to_rgb("#F5C2E7"), Some((0xf5, 0xc2, 0xe7)));
        assert_eq!(to_rgb("#abc"), Some((0xaa, 0xbb, 0xcc)));
        assert_eq!(to_rgb("rgb(137,180,250)"), Some((137, 180, 250)));
        assert_eq!(to_rgb("cyan"), Some((6, 152, 154)));
    }

    #[test]
    fn what_cannot_be_previewed_returns_none() {
        for none in ["reset", "", "rosewater", "#12345"] {
            assert_eq!(to_rgb(none), None, "{none}");
        }
    }

    #[test]
    fn hsv_round_trips_through_rgb() {
        // Every channel of the app's own palette must survive the trip, or
        // opening the picker would shift the colour.
        for hex in [
            "#11111b", "#181825", "#1e1e2e", "#313244", "#cdd6f4", "#7f849c", "#45475a", "#89b4fa",
            "#a6e3a1", "#f9e2af", "#f38ba8", "#ffffff", "#000000", "#ff0000",
        ] {
            let (r, g, b) = parse_hex(hex).unwrap();
            let (h, s, v) = rgb_to_hsv(r, g, b);
            assert_eq!(hsv_to_rgb(h, s, v), (r, g, b), "{hex}");
        }
    }

    #[test]
    fn the_hue_ring_covers_every_sector() {
        // Six sectors, so a bug in one shows up as a wrong primary.
        let at = |h: f32| hsv_to_rgb(h, 1.0, 1.0);
        assert_eq!(at(0.0), (255, 0, 0));
        assert_eq!(at(1.0 / 6.0), (255, 255, 0));
        assert_eq!(at(2.0 / 6.0), (0, 255, 0));
        assert_eq!(at(3.0 / 6.0), (0, 255, 255));
        assert_eq!(at(4.0 / 6.0), (0, 0, 255));
        assert_eq!(at(5.0 / 6.0), (255, 0, 255));
        assert_eq!(at(1.0), (255, 0, 0), "the ring wraps");
    }

    #[test]
    fn grey_has_no_hue_and_black_has_no_value() {
        let (_, s, v) = rgb_to_hsv(128, 128, 128);
        assert_eq!(s, 0.0);
        assert!((v - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(rgb_to_hsv(0, 0, 0), (0.0, 0.0, 0.0));
    }
}
