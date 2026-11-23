use gpui_kit::*;

/// Parse a six-digit `#rrggbb` color. Callers choose how to handle invalid input.
pub(crate) fn parse_hex_color(value: &str) -> Option<Hsla> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16)
        .ok()
        .map(|color| rgb(color).into())
}

#[cfg(test)]
mod tests {
    use super::parse_hex_color;

    #[test]
    fn parses_six_digit_hex_with_or_without_a_hash() {
        assert!(parse_hex_color("#FF0000").is_some());
        assert!(parse_hex_color("00ffff").is_some());
    }

    #[test]
    fn rejects_short_and_invalid_colors() {
        assert!(parse_hex_color("#fff").is_none());
        assert!(parse_hex_color("#gggggg").is_none());
    }
}
