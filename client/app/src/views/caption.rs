use crate::platform::window;
use crate::view_model::ClientViewModel;
use cynapse_client_core::caption_segments;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Duration;

use super::color::parse_hex_color;

/// Hover latency and wakeup budget while a caption is visible.
const HOVER_POLL_INTERVAL: Duration = Duration::from_millis(50);

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
            current_height,
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
                view_model.caption_height(),
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
        if !plain.is_empty() {
            let available_width = (window.bounds().size.width - px(64.)).max(px(80.));
            if let Ok(lines) = window.text_system().shape_text(
                plain.clone().into(),
                px(font_size),
                &runs,
                Some(available_width),
                None,
            ) {
                let measured = lines
                    .iter()
                    .map(|line| line.size(line_height).height.as_f32())
                    .sum::<f32>()
                    + 32.0;
                if (measured - current_height as f32).abs() > 1.0 {
                    tracing::debug!(
                        target: "cynapse.client.caption",
                        measured,
                        current_height,
                        font_size,
                        "caption content height changed"
                    );
                    let view_model = self.view_model.clone();
                    cx.defer(move |cx| {
                        view_model.update(cx, |view_model, cx| {
                            view_model.set_caption_height(measured, cx);
                        });
                    });
                }
            }
        }

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .py_2()
            .child({
                let card = div().max_w_full().rounded_md().border_0().px_4().py_2();
                let card = if let Some(color) = background_color {
                    card.bg(color.opacity(background_opacity))
                } else {
                    card
                };
                card.child(
                    div()
                        .text_center()
                        .text_size(px(font_size))
                        .line_height(line_height)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_color)
                        .opacity(text_opacity)
                        .when_some(font_family, |text, family| text.font_family(family))
                        .child(gpui::StyledText::new(plain).with_highlights(highlights)),
                )
            })
    }
}
