use crate::platform::window;
use crate::view_model::ClientViewModel;
use cynapse_client_core::{CaptionPosition, caption_segments};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Duration;

use super::color::parse_hex_color;

mod formula;

/// Hover latency and wakeup budget while a caption is visible.
const HOVER_POLL_INTERVAL: Duration = Duration::from_millis(50);

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
    let text_color =
        parse_hex_color(&params.text_color).unwrap_or_else(|| rgb(0xffffff).into());
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

    let text_style = window.text_style();
    let mut runs = Vec::new();
    let mut offset = 0;
    for segment in &segments {
        let end = offset + segment.text.len();
        let mut font = text_style.font();
        if let Some(family) = params.font_family.as_deref() {
            font.family = family.into();
        }
        if segment.aqua {
            font.weight = gpui::FontWeight::BOLD;
        }
        runs.push(gpui::TextRun {
            len: segment.text.len(),
            font,
            color: if segment.aqua { accent_color } else { text_color },
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        offset = end;
    }

    let line_height = px(params.font_size * 1.3);
    let monitor_width = window
        .display(cx)
        .map(|display| display.visible_bounds().size.width.as_f32())
        .unwrap_or(1920.0);
    let max_card_width = px(crate::caption_metrics::max_card_width(
        monitor_width,
        params.width_percent,
    ));

    let mixed = segments.iter().any(|segment| segment.formula).then(|| {
        formula::layout(
            &segments,
            &runs,
            px(params.font_size),
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
        let natural = plain
            .split('\n')
            .map(|line| {
                text_system
                    .shape_line(line.to_owned().into(), px(params.font_size), &runs, None)
                    .width()
            })
            .fold(px(0.), Pixels::max);
        let card_width = natural.min(max_card_width);
        let height: Pixels = text_system
            .shape_text(
                plain.clone().into(),
                px(params.font_size),
                &runs,
                Some(card_width),
                None,
            )
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
            segments,
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
                caption_segments(view_model.caption_text()),
                view_model.caption_font_size(),
                view_model.caption_width_percent(),
                view_model.caption_position(),
                view_model.caption_font_family().family_name(),
            )
        };
        let plain = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>();
        let text_style = window.text_style();
        let mut highlights = Vec::new();
        let mut runs = Vec::new();
        let mut offset = 0;
        for segment in &segments {
            let end = offset + segment.text.len();
            if segment.aqua {
                highlights.push((
                    offset..end,
                    gpui::HighlightStyle {
                        color: Some(accent_color),
                        font_weight: Some(gpui::FontWeight::BOLD),
                        ..Default::default()
                    },
                ));
            }
            let mut font = text_style.font();
            if let Some(font_family) = &font_family {
                font.family = (*font_family).into();
            }
            if segment.aqua {
                font.weight = gpui::FontWeight::BOLD;
            }
            runs.push(gpui::TextRun {
                len: segment.text.len(),
                font,
                color: if segment.aqua {
                    accent_color
                } else {
                    text_color
                },
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            offset = end;
        }

        let line_height = px(font_size * 1.3);
        let monitor_width = window
            .display(cx)
            .map(|display| display.visible_bounds().size.width.as_f32())
            .unwrap_or(1920.0);
        // Settings percentage is the hard ceiling for the card width.
        let max_card_width = px(crate::caption_metrics::max_card_width(monitor_width, width_percent));

        // Measure the true text size: natural (unwrapped) glyph width determines
        // the card width; wrapping at that width gives the height. The window
        // width is fixed, so this measured card width is stable frame to frame.
        let mixed = segments.iter().any(|segment| segment.formula).then(|| {
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
        let card_size = if let Some(layout) = &mixed {
            Some((layout.width(), layout.height()))
        } else if plain.is_empty() {
            None
        } else {
            let text_system = window.text_system();
            let natural = plain
                .split('\n')
                .map(|line| {
                    text_system
                        .shape_line(line.to_owned().into(), px(font_size), &runs, None)
                        .width()
                })
                .fold(px(0.), Pixels::max);
            let card_width = natural.min(max_card_width);
            let height: Pixels = text_system
                .shape_text(
                    plain.clone().into(),
                    px(font_size),
                    &runs,
                    Some(card_width),
                    None,
                )
                .map(|lines| lines.iter().map(|l| l.size(line_height).height).sum())
                .unwrap_or(line_height);
            Some((card_width, height))
        };
        // Card width for rendering only. The window height was measured before
        // the window was shown (see show_caption flow), so no height update or
        // transparent placeholder frame is needed here.
        let card_width_px = card_size.map(|(width, _)| width.as_f32());

        // The window stays transparent; the inner card hugs the measured text
        // width instead of filling the window. w() includes padding, so add the
        // 16px back to keep the content box equal to the measured width.
        let card = div()
            .when_some(card_width_px, |el, width| {
                el.w(px(width + crate::caption_metrics::CAPTION_PADDING))
            })
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .p_2();
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
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text_color)
                .opacity(text_opacity)
                .when_some(font_family, |text, family| text.font_family(family))
                .child(if let Some(layout) = mixed {
                    layout.into_element(plain)
                } else {
                    gpui::StyledText::new(plain)
                        .with_highlights(highlights)
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
