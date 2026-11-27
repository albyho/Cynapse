use crate::platform::window;
use crate::view_model::ClientViewModel;
use cynapse_client_core::{CaptionPosition, CaptionSegment, caption_segments};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Duration;

use super::color::parse_hex_color;

mod formula;

/// Hover latency and wakeup budget while a caption is visible.
const HOVER_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn caption_runs(
    segments: &[CaptionSegment],
    mut font: Font,
    font_family: Option<&str>,
    text_color: Hsla,
    accent_color: Hsla,
) -> Vec<TextRun> {
    if let Some(family) = font_family {
        font.family = family.to_owned().into();
    }
    font.weight = FontWeight::SEMIBOLD;
    segments
        .iter()
        .map(|segment| {
            let mut font = font.clone();
            if segment.emphasized {
                font.weight = FontWeight::BOLD;
            }
            TextRun {
                len: segment.text.len(),
                font,
                color: if segment.emphasized {
                    accent_color
                } else {
                    text_color
                },
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        })
        .collect()
}

fn plain_card_width(
    window: &Window,
    plain: SharedString,
    font_size: Pixels,
    runs: &[TextRun],
    max_width: Pixels,
) -> Pixels {
    // Shape the full string so each hard line gets its own span styles.
    window
        .text_system()
        .shape_text(plain, font_size, runs, None, None)
        .map(|lines| {
            let natural = lines
                .iter()
                .map(|line| line.size(px(0.)).width)
                .fold(px(0.), Pixels::max);
            // Round up to a device pixel, then add one logical pixel of slack so
            // GPUI's layout rounding does not force the last word onto another line.
            let scale = window.scale_factor();
            let device_aligned = (natural * scale).ceil() / scale + px(1.0);
            (device_aligned + snapped_padding(window) - px(crate::caption_metrics::CAPTION_PADDING))
                .ceil()
        })
        .unwrap_or(max_width)
        .min(max_width.floor())
}

fn snapped_padding(window: &Window) -> Pixels {
    window.pixel_snap(px(crate::caption_metrics::CAPTION_PADDING / 2.)) * 2.
}

/// Measure the caption's content width and the resulting window height.
///
/// This is a pure measurement that only needs a window for GPUI's text system
/// and SVG renderer. It is called BEFORE the caption window is shown, using the
/// main window's context, so the caption window can be sized correctly on its
/// very first visible frame (no flash/resize cycle).
pub(crate) fn measure_caption(
    params: &crate::caption_metrics::CaptionMeasureParams,
    window: &mut Window,
    cx: &App,
) -> Option<(f32, f32)> {
    let text_color = parse_hex_color(&params.text_color).unwrap_or_else(|| rgb(0xffffff).into());
    let accent_color =
        parse_hex_color(&params.accent_color).unwrap_or_else(|| rgb(0x00ffff).into());
    let segments = caption_segments(&params.text);
    if segments.is_empty() {
        return None;
    }
    let plain = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();

    let runs = caption_runs(
        &segments,
        window.text_style().font(),
        params.font_family.as_deref(),
        text_color,
        accent_color,
    );
    let font_size = px(params.font_size);
    let line_height = window.pixel_snap(px(params.font_size * 1.3));
    let monitor_width = window
        .display(cx)
        .map(|display| display.visible_bounds().size.width.as_f32())
        .unwrap_or(1920.0);
    let max_card_width = px(crate::caption_metrics::max_card_width(
        monitor_width,
        params.width_percent,
    ));

    let mixed =
        (params.formulas_enabled && segments.iter().any(|segment| segment.formula)).then(|| {
            formula::layout(
                &segments,
                &runs,
                font_size,
                line_height,
                max_card_width,
                window,
                cx,
            )
        });
    let card_size = if let Some(layout) = &mixed {
        (layout.width(), layout.height())
    } else {
        let text_system = window.text_system();
        let plain: SharedString = plain.into();
        let card_width = plain_card_width(window, plain.clone(), font_size, &runs, max_card_width);
        let wrap_width = window
            .pixel_snap(card_width + px(crate::caption_metrics::CAPTION_PADDING))
            - snapped_padding(window);
        let height: Pixels = text_system
            .shape_text(plain, font_size, &runs, Some(wrap_width), None)
            .map(|lines| lines.iter().map(|l| l.size(line_height).height).sum())
            .unwrap_or(line_height);
        (card_width, height)
    };

    Some(crate::caption_metrics::content_to_window_size(
        card_size.0.as_f32(),
        card_size.1.as_f32(),
        line_height.as_f32(),
    ))
}

/// Declarative caption-overlay view.
///
/// The window is transparent and click-through at the platform boundary; this
/// view paints only the caption background and text.
pub struct CaptionView {
    view_model: Entity<ClientViewModel>,
    hover_task: Option<Task<()>>,
    _subscription: Subscription,
}

impl CaptionView {
    pub fn new(view_model: Entity<ClientViewModel>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&view_model, |this, _, cx| {
            this.sync_hover_poll(cx);
            cx.notify();
        });
        let mut view = Self {
            view_model,
            hover_task: None,
            _subscription: subscription,
        };
        view.sync_hover_poll(cx);
        view
    }

    /// Poll the cursor only while a caption is visible. Hiding drops the task,
    /// so an idle app does not wake up just to check for hover.
    fn sync_hover_poll(&mut self, cx: &mut Context<Self>) {
        if !self.view_model.read(cx).caption_visible() {
            self.hover_task = None;
            return;
        }
        if self.hover_task.is_some() {
            return;
        }
        self.hover_task = Some(cx.spawn(async move |this, cx| {
            let mut last_rect = None;
            let mut inside = false;
            loop {
                cx.background_executor().timer(HOVER_POLL_INTERVAL).await;
                let (visible, rect) = this
                    .update(cx, |this, cx| {
                        let view_model = this.view_model.read(cx);
                        (view_model.caption_visible(), view_model.caption_rect())
                    })
                    .unwrap_or((false, None));
                if !visible {
                    break;
                }
                let Some(rect) = rect else {
                    last_rect = None;
                    inside = false;
                    continue;
                };
                let now_inside = window::cursor_over_rect(rect);
                if last_rect != Some(rect) {
                    // A new or moved caption under the pointer is not a hover
                    // entry; only leaving and entering again toggles the edge.
                    last_rect = Some(rect);
                    inside = now_inside;
                    continue;
                }
                if now_inside && !inside {
                    let _ = this.update(cx, |this, cx| {
                        this.view_model
                            .update(cx, |view_model, cx| view_model.toggle_caption_edge(cx));
                    });
                }
                inside = now_inside;
            }
        }));
    }
}

impl Render for CaptionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (
            background_color,
            background_opacity,
            text_opacity,
            text_color,
            accent_color,
            caption,
            formulas_enabled,
            font_size,
            width_percent,
            position,
            font_family,
        ) = {
            let view_model = self.view_model.read(cx);
            (
                if view_model.caption_background_enabled() {
                    view_model
                        .caption_background_color()
                        .and_then(parse_hex_color)
                } else {
                    None
                },
                view_model.caption_background_opacity().clamp(0.0, 1.0),
                view_model.caption_text_opacity().clamp(0.0, 1.0),
                parse_hex_color(view_model.caption_text_color())
                    .unwrap_or_else(|| rgb(0xffffff).into()),
                parse_hex_color(view_model.caption_accent_color())
                    .unwrap_or_else(|| rgb(0x00ffff).into()),
                view_model.caption_text().to_owned(),
                view_model.caption_formulas_enabled(),
                view_model.caption_font_size(),
                view_model.caption_width_percent(),
                view_model.caption_position(),
                view_model.caption_font_family().family_name(),
            )
        };
        let segments = caption_segments(&caption);
        let plain = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>();
        if plain.is_empty() {
            return Empty.into_any_element();
        }
        let runs = caption_runs(
            &segments,
            window.text_style().font(),
            font_family,
            text_color,
            accent_color,
        );
        let line_height = window.pixel_snap(px(font_size * 1.3));
        let monitor_width = window
            .display(cx)
            .map(|display| display.visible_bounds().size.width.as_f32())
            .unwrap_or(1920.0);
        // Settings percentage is the hard ceiling for the card width.
        let max_card_width = px(crate::caption_metrics::max_card_width(
            monitor_width,
            width_percent,
        ));

        let mixed =
            (formulas_enabled && segments.iter().any(|segment| segment.formula)).then(|| {
                formula::layout(
                    &segments,
                    &runs,
                    px(font_size),
                    line_height,
                    max_card_width,
                    window,
                    cx,
                )
            });
        let card_width = if let Some(layout) = &mixed {
            layout.width()
        } else {
            plain_card_width(
                window,
                plain.clone().into(),
                px(font_size),
                &runs,
                max_card_width,
            )
        };

        // The window stays transparent; the inner card hugs the measured text
        // width instead of filling the window. w() includes padding, so add the
        // 16px back to keep the content box equal to the measured width.
        let card = div()
            .id("caption-card")
            .test_support()
            .w(card_width + px(crate::caption_metrics::CAPTION_PADDING))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .p(px(crate::caption_metrics::CAPTION_PADDING / 2.));
        let card = if let Some(color) = background_color {
            card.bg(color.opacity(background_opacity))
        } else {
            card
        };
        let card = card.child(
            div()
                .w_full()
                .text_center()
                .text_size(px(font_size))
                .line_height(line_height)
                .opacity(text_opacity)
                .child(if let Some(layout) = mixed {
                    let view_model = self.view_model.clone();
                    layout.into_element(plain, move |cx| {
                        view_model.update(cx, |view_model, cx| {
                            view_model.show_caption_formula_source(&caption, cx);
                        });
                    })
                } else {
                    gpui::StyledText::new(plain)
                        .with_runs(runs)
                        .into_any_element()
                }),
        );
        // Pin the card to the window edge matching the screen edge. While the
        // first (unmeasured) frame uses a full-screen-height window, this keeps
        // the card at the correct edge instead of flashing in the center.
        let root = div().size_full().flex().justify_center();
        let root = match position {
            CaptionPosition::Bottom => root.items_end(),
            CaptionPosition::Top => root.items_start(),
        };
        root.child(card).into_any_element()
    }
}
