//! Inline math shares a baseline and measured line box with shaped Caption text.

use cynapse_client_core::{CaptionSegment, caption_formula_layout};
use gpui_kit::prelude::*;
use gpui_kit::*;
use ratex_svg::{SvgColorSyntax, SvgOptions, render_to_svg_with_color_syntax};
use std::sync::Arc;
use unicode_linebreak::{BreakOpportunity, linebreaks};

struct Item {
    text: ShapedLine,
    image: Option<Arc<RenderImage>>,
    width: Pixels,
    ascent: Pixels,
    descent: Pixels,
}

#[derive(Default)]
struct Line {
    items: Vec<Item>,
    width: Pixels,
    ascent: Pixels,
    descent: Pixels,
}

impl Line {
    fn push(&mut self, item: Item) {
        self.width += item.width;
        self.ascent = self.ascent.max(item.ascent);
        self.descent = self.descent.max(item.descent);
        self.items.push(item);
    }
}

pub(super) struct Layout {
    lines: Vec<Line>,
    line_height: Pixels,
}

impl Layout {
    pub(super) fn height(&self) -> Pixels {
        self.lines
            .iter()
            .map(|line| self.line_height.max(line.ascent + line.descent))
            .sum()
    }

    pub(super) fn width(&self) -> Pixels {
        self.lines
            .iter()
            .map(|line| line.width)
            .fold(px(0.), Pixels::max)
    }

    pub(super) fn into_element(self, source: String) -> AnyElement {
        let height = self.height();
        div()
            .id("caption-formulas")
            .test_support()
            .role(Role::Label)
            .aria_label(source)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        tracing::warn!(
                            target: "cynapse.client.caption",
                            bounds_w = bounds.size.width.as_f32(),
                            bounds_h = bounds.size.height.as_f32(),
                            line_count = self.lines.len(),
                            "formula canvas paint"
                        );
                        let mut y = bounds.origin.y;
                        for line in self.lines {
                            let height = self.line_height.max(line.ascent + line.descent);
                            let baseline =
                                y + (height - line.ascent - line.descent) / 2. + line.ascent;
                            let mut x = bounds.origin.x + (bounds.size.width - line.width) / 2.;
                            for item in line.items {
                                let painted = item.image.is_some_and(|image| {
                                    let bounds = Bounds::new(
                                        point(x, baseline - item.ascent),
                                        size(item.width, item.ascent + item.descent),
                                    );
                                    window
                                        .paint_image(
                                            bounds,
                                            bounds,
                                            Default::default(),
                                            image,
                                            0,
                                            false,
                                        )
                                        .is_ok()
                                });
                                if !painted {
                                    let _ = item.text.paint(
                                        point(x, baseline - item.text.ascent),
                                        item.text.ascent + item.text.descent,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    );
                                }
                                x += item.width;
                            }
                            y += height;
                        }
                    },
                )
                .w_full()
                .h(height),
            )
            .into_any_element()
    }
}

pub(super) fn layout(
    segments: &[CaptionSegment],
    runs: &[TextRun],
    font_size: Pixels,
    line_height: Pixels,
    max_width: Pixels,
    window: &Window,
    cx: &App,
) -> Layout {
    let mut lines = vec![Line::default()];
    let mut push = |item: Item, hard_break: bool| {
        let line = lines.last_mut().expect("at least one Caption line");
        if !line.items.is_empty() && line.width + item.width > max_width {
            lines.push(Line::default());
        }
        lines
            .last_mut()
            .expect("at least one Caption line")
            .push(item);
        if hard_break {
            lines.push(Line::default());
        }
    };
    for (segment, run) in segments.iter().zip(runs) {
        let shape = |source: &str| {
            window.text_system().shape_line(
                source.to_owned().into(),
                font_size,
                &[TextRun {
                    len: source.len(),
                    ..run.clone()
                }],
                None,
            )
        };
        if segment.formula
            && let Ok(formula) = caption_formula_layout(&segment.text)
        {
            let em = f64::from(font_size.as_f32());
            let padding = 0.5;
            let scale = em.min((f64::from(max_width.as_f32()) - 2. * padding) / formula.width);
            let metrics = |dimension: f64| px((dimension * scale + padding) as f32);
            let rgba = Rgba::from(run.color);
            let channel = |value: f32| (value * 255.).round() as u8;
            let ink = format!(
                "rgb({},{},{})",
                channel(rgba.r),
                channel(rgba.g),
                channel(rgba.b)
            );
            let svg = render_to_svg_with_color_syntax(
                &ratex_layout::to_display_list(&formula),
                &SvgOptions {
                    font_size: scale,
                    padding,
                    embed_glyphs: true,
                    ..Default::default()
                },
                SvgColorSyntax::Rgb,
            )
            .replace("rgb(0,0,0)", &ink);
            let image = cx
                .svg_renderer()
                .render_single_frame(svg.as_bytes(), window.scale_factor())
                .ok()
                .filter(|image| {
                    image.as_bytes(0).is_some_and(|bytes| {
                        bytes.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0)
                    })
                });
            if let Some(image) = image {
                let text = shape(&segment.text.replace(['\r', '\n'], " "));
                push(
                    Item {
                        text,
                        image: Some(image),
                        width: metrics(formula.width) + px(padding as f32),
                        ascent: metrics(formula.height),
                        descent: metrics(formula.depth),
                    },
                    false,
                );
                continue;
            }
        }

        let mut start = 0;
        for (end, opportunity) in linebreaks(&segment.text) {
            let source = &segment.text[start..end];
            start = end;
            let mut text = shape(source.trim_end_matches(['\r', '\n']));
            while let Some(index) = text.index_for_x(max_width).filter(|index| *index > 0) {
                let (left, rest) = text.split_at(index);
                push(
                    Item {
                        width: left.width,
                        ascent: left.ascent,
                        descent: left.descent,
                        text: left,
                        image: None,
                    },
                    true,
                );
                text = rest;
            }
            push(
                Item {
                    width: text.width,
                    ascent: text.ascent,
                    descent: text.descent,
                    text,
                    image: None,
                },
                opportunity == BreakOpportunity::Mandatory
                    && (end != segment.text.len() || source.ends_with('\n')),
            );
        }
    }
    Layout { lines, line_height }
}

#[cfg(test)]
mod tests {
    use super::layout;
    use cynapse_client_core::caption_segments;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Context, IntoElement, Render, TestAppContext, TextRun, Window, px, size,
    };

    struct CaptionExample;

    impl Render for CaptionExample {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let source = r"递推 \(T(n)=\begin{cases}1&n=1\\2T(n/2)+n&n>1\end{cases}\) 完毕";
            let segments = caption_segments(source);
            let style = window.text_style();
            let runs: Vec<_> = segments
                .iter()
                .map(|segment| TextRun {
                    len: segment.text.len(),
                    font: style.font(),
                    color: style.color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                })
                .collect();
            layout(&segments, &runs, px(24.), px(32.), px(330.), window, cx)
                .into_element(source.into())
        }
    }

    #[gpui_kit::test]
    fn formula_caption_reserves_height_and_exposes_source(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(400.), px(200.)), |_, _| CaptionExample);
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let caption = window.find("caption-formulas");
            assert!(caption.bounds().size.height > px(64.));
            assert!(caption.label().unwrap().contains(r"递推 \(T(n)="));
        })
        .expect("formula Caption window");
    }
}
