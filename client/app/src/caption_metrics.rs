//! Pure caption sizing math, shared by measurement and placement.
//!
//! Keeping these calculations free of GPUI window/display handles lets them be
//! unit-tested directly. The caption card's padding is defined once here so the
//! measured content width and the rendered content box cannot drift apart.

use cynapse_client_core::CaptionPosition;

/// Padding on every side of the caption card, in logical pixels.
pub const CAPTION_PADDING: f32 = 16.0;
/// Minimum window width used when the Settings ceiling is very small.
pub const CAPTION_MIN_WIDTH: f32 = 320.0;
/// First-frame height before the first caption has been measured.
pub const CAPTION_MIN_HEIGHT: f32 = 96.0;
/// Gap between the caption window and the screen edge, in logical pixels.
pub const CAPTION_EDGE_MARGIN: f32 = 4.0;

/// Extra vertical room absorbs glyph-box rounding between measurement and paint.
const VERTICAL_MARGIN_RATIO: f32 = 0.2;
const VERTICAL_MARGIN_MIN: f32 = 4.0;
/// Vertical space kept free when clamping the window to the display.
const DISPLAY_VERTICAL_GUARD: f32 = 80.0;

/// Inputs for measuring a caption, gathered from the view model before the
/// caption window is shown. Kept as one value so the measurement function takes
/// only the params plus the GPUI window/context.
pub struct CaptionMeasureParams {
    pub text: String,
    pub font_size: f32,
    pub font_family: Option<String>,
    pub text_color: String,
    pub accent_color: String,
    pub width_percent: u8,
    pub formulas_enabled: bool,
}

/// Widest card text allowed by the Settings percentage, after card padding.
///
/// The percentage is a ceiling, not a fixed width: short text wraps less and
/// yields a narrower card.
pub fn max_card_width(visible_width: f32, width_percent: u8) -> f32 {
    (visible_width * f32::from(width_percent.clamp(50, 100)) / 100.0 - CAPTION_PADDING).max(80.0)
}

/// Convert measured card content size into the window size.
///
/// Padding is applied exactly once, here. Both the measurement path and the
/// renderer derive their width from this so the content box matches the text
/// that was measured.
pub fn content_to_window_size(
    content_width: f32,
    content_height: f32,
    line_height: f32,
) -> (f32, f32) {
    let width = content_width + CAPTION_PADDING;
    let height = content_height
        + CAPTION_PADDING
        + (line_height * VERTICAL_MARGIN_RATIO).max(VERTICAL_MARGIN_MIN);
    (width, height)
}

/// The logical-pixel window rectangle: `(x, y, width, height)`.
///
/// The window hugs the measured size, clamped to the Settings width ceiling and
/// the display height. It is horizontally centered and pinned to the matching
/// screen edge, so a full-height first frame never paints in the center.
pub fn caption_rect_logical(
    origin: (f32, f32),
    visible_size: (f32, f32),
    width_percent: u8,
    measured: (u32, u32),
    position: CaptionPosition,
) -> (f32, f32, f32, f32) {
    let max_width =
        (visible_size.0 * f32::from(width_percent.clamp(50, 100)) / 100.0).max(CAPTION_MIN_WIDTH);
    let max_height = (visible_size.1 - DISPLAY_VERTICAL_GUARD).max(1.0);
    let width = (measured.0 as f32).max(1.0).min(max_width);
    let height = (measured.1 as f32).max(1.0).min(max_height);

    let x = origin.0 + (visible_size.0 - width) / 2.0;
    let y = match position {
        CaptionPosition::Bottom => origin.1 + visible_size.1 - height - CAPTION_EDGE_MARGIN,
        CaptionPosition::Top => origin.1 + CAPTION_EDGE_MARGIN,
    };
    (x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_card_width_scales_with_percentage_and_leaves_padding() {
        // 1000px display: 80% = 800, minus 16px card padding.
        assert!((max_card_width(1000.0, 80) - 784.0).abs() < 0.01);
        assert!((max_card_width(1000.0, 50) - 484.0).abs() < 0.01);
    }

    #[test]
    fn max_card_width_clamps_percentage_and_floors() {
        // Out-of-range percentages clamp; tiny displays still leave room.
        assert_eq!(max_card_width(1000.0, 20), max_card_width(1000.0, 50));
        assert_eq!(max_card_width(1000.0, 200), max_card_width(1000.0, 100));
        assert!(max_card_width(10.0, 80) >= 80.0);
    }

    #[test]
    fn window_size_adds_padding_and_vertical_margin() {
        // line_height 40 -> margin max(8, 4) = 8.
        let (w, h) = content_to_window_size(100.0, 60.0, 40.0);
        assert!((w - 116.0).abs() < 0.01);
        assert!((h - 84.0).abs() < 0.01);
    }

    #[test]
    fn window_size_uses_minimum_vertical_margin_for_small_text() {
        // line_height 5 -> margin max(1, 4) = 4.
        let (_, h) = content_to_window_size(50.0, 20.0, 5.0);
        assert!((h - 40.0).abs() < 0.01);
    }

    #[test]
    fn short_text_shrinks_below_the_settings_ceiling() {
        // Measured 200px on a 2000px/80% display -> stays 200px, not 1600px.
        let rect = caption_rect_logical(
            (0.0, 0.0),
            (2000.0, 1000.0),
            80,
            (200, 60),
            CaptionPosition::Bottom,
        );
        assert_eq!(rect.2, 200.0);
        assert_eq!(rect.3, 60.0);
    }

    #[test]
    fn long_text_is_clamped_to_the_settings_ceiling() {
        // 2000px * 80% = 1600px ceiling; measured 1800px is clamped.
        let rect = caption_rect_logical(
            (0.0, 0.0),
            (2000.0, 1000.0),
            80,
            (1800, 200),
            CaptionPosition::Bottom,
        );
        assert_eq!(rect.2, 1600.0);
    }

    #[test]
    fn window_is_horizontally_centered() {
        let rect = caption_rect_logical(
            (0.0, 0.0),
            (2000.0, 1000.0),
            80,
            (400, 60),
            CaptionPosition::Bottom,
        );
        // x + width/2 == display center.
        assert!((rect.0 + rect.2 / 2.0 - 1000.0).abs() < 0.01);
    }

    #[test]
    fn bottom_and_top_pin_to_matching_edges() {
        let bottom = caption_rect_logical(
            (0.0, 0.0),
            (2000.0, 1000.0),
            80,
            (400, 60),
            CaptionPosition::Bottom,
        );
        // Bottom edge sits CAPTION_EDGE_MARGIN above the display bottom.
        assert!((bottom.1 + bottom.3 - (1000.0 - CAPTION_EDGE_MARGIN)).abs() < 0.01);

        let top = caption_rect_logical(
            (0.0, 0.0),
            (2000.0, 1000.0),
            80,
            (400, 60),
            CaptionPosition::Top,
        );
        assert!((top.1 - CAPTION_EDGE_MARGIN).abs() < 0.01);
    }

    #[test]
    fn repeated_measurement_is_stable() {
        // Measuring the same content twice yields the same size: no oscillation.
        let first = content_to_window_size(300.0, 80.0, 40.0);
        let second = content_to_window_size(300.0, 80.0, 40.0);
        assert_eq!(first, second);
    }

    #[test]
    fn changing_percentage_changes_width() {
        let narrow = caption_rect_logical(
            (0.0, 0.0),
            (2000.0, 1000.0),
            50,
            (1800, 200),
            CaptionPosition::Bottom,
        );
        let wide = caption_rect_logical(
            (0.0, 0.0),
            (2000.0, 1000.0),
            80,
            (1800, 200),
            CaptionPosition::Bottom,
        );
        assert!(wide.2 > narrow.2);
    }
}
