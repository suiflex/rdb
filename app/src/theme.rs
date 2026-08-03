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
    let rgb = u32::from_str_radix(h, 16).ok()?;
    Some(Color::from_rgb_u8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
    ))
}

/// Parse with a fallback to the design-system accent (Tokens.accent, #2c5fd8).
pub fn accent_or_default(s: &str) -> Color {
    parse_hex(s).unwrap_or_else(|| Color::from_rgb_u8(0x2c, 0x5f, 0xd8))
}

/// Fixed environment-tag -> color mapping (5 of the 8 `Swatches` hexes from
/// conn-form.slint, kept identical there via `EnvSwatches` so the dialog
/// picker and the sidebar pill agree). `EnvTag::None` renders no pill.
pub fn env_tag_color(tag: rdb_connstore::EnvTag) -> Option<Color> {
    use rdb_connstore::EnvTag::*;
    match tag {
        None => Option::None,
        Local => parse_hex("#64748b"),
        Dev => parse_hex("#239d5c"),
        Staging => parse_hex("#d9962f"),
        Testing => parse_hex("#a855f7"),
        Production => parse_hex("#e05a4e"),
    }
}

/// Pill label for an environment tag; empty string means "render no pill".
pub fn env_tag_label(tag: rdb_connstore::EnvTag) -> &'static str {
    use rdb_connstore::EnvTag::*;
    match tag {
        None => "",
        Local => "LOCAL",
        Dev => "DEV",
        Staging => "STAGING",
        Testing => "TESTING",
        Production => "PRODUCTION",
    }
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
