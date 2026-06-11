//! Accent color helpers: connstore stores the per-connection color as a hex
//! string; the UI needs a `slint::Color`.

use slint::Color;

/// Parse "#rrggbb" or "rrggbb" into a Slint Color. Returns None on bad input
/// so the caller can fall back to the default accent.
pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color::from_rgb_u8(r, g, b))
}

/// Parse with a fallback to the spec's default accent (#3b82f6).
pub fn accent_or_default(s: &str) -> Color {
    parse_hex(s).unwrap_or_else(|| Color::from_rgb_u8(0x3b, 0x82, 0xf6))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_digit_hex() {
        let c = parse_hex("#3b82f6").unwrap();
        assert_eq!((c.red(), c.green(), c.blue()), (0x3b, 0x82, 0xf6));
    }

    #[test]
    fn parses_without_leading_hash() {
        assert!(parse_hex("ff0000").is_some());
    }

    #[test]
    fn rejects_garbage_returns_none() {
        assert!(parse_hex("nope").is_none());
        assert!(parse_hex("#12").is_none());
    }
}
