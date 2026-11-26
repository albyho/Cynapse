//! Shared full-window busy overlay for reset and the generic Start Learning flow.
//!
//! The overlay owns only presentation state. The caller supplies an optional
//! step list; reset uses only the label, while Start Learning shows the real
//! startup stages so a slow first run does not look frozen.

use cynapse_client_i18n::t;
use std::time::Duration;

use gpui_kit::base::motion::{Presence, Transition};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::stepper::{Stepper, StepperItem};
use gpui_kit::component::{ActiveTheme, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, Styled,
    TestSupportExt, Window, div, px,
};

const FADE_IN: Duration = Duration::from_millis(120);
const FADE_OUT: Duration = Duration::from_millis(180);

#[derive(Clone, Default, PartialEq, Eq)]
pub struct BusyOverlayContent {
    pub label: Option<String>,
    pub steps: Vec<String>,
    pub active_step: Option<usize>,
    pub step_details: Vec<Option<String>>,
    pub hint: Option<String>,
    pub cancel_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusyOverlayEvent {
    Cancel,
}

impl EventEmitter<BusyOverlayEvent> for BusyOverlay {}

pub struct BusyOverlay {
    visible: bool,
    has_been_visible: bool,
    content: BusyOverlayContent,
}

impl Default for BusyOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl BusyOverlay {
    pub fn new() -> Self {
        Self {
            visible: false,
            has_been_visible: false,
            content: BusyOverlayContent {
                label: Some(t("reset.default_label")),
                ..Default::default()
            },
        }
    }

    pub fn sync(&mut self, visible: bool, content: BusyOverlayContent, cx: &mut Context<Self>) {
        if self.update_state(visible, content) {
            cx.notify();
        }
    }

    fn update_state(&mut self, visible: bool, mut content: BusyOverlayContent) -> bool {
        content.label = match content.label {
            Some(label) if !label.is_empty() => Some(label),
            _ if visible => Some(t("reset.default_label")),
            _ => self.content.label.clone(),
        };
        let changed = self.visible != visible || self.content != content;
        self.visible = visible;
        if visible {
            self.has_been_visible = true;
        }
        self.content = content;
        changed
    }
}

impl Render for BusyOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let transition = Transition::new(if self.visible { FADE_IN } else { FADE_OUT });
        let presence = Presence::new("busy-overlay", self.visible)
            .transition(transition)
            .sample(window, cx);
        if !self.has_been_visible || !presence.should_render() || presence.progress <= 0.0 {
            return div().into_any_element();
        }

        let active = self.content.active_step.unwrap_or(0);
        let steps = self
            .content
            .steps
            .iter()
            .enumerate()
            .map(|(index, label)| {
                // Stepper triggers align children to the top; match the 18px
                // indicator height so the label is vertically centered with it.
                let label = if self.content.active_step == Some(index) {
                    ShimmerText::new(label.clone())
                        .duration(Duration::from_secs(3))
                        .spread(0.25)
                        .into_any_element()
                } else {
                    div().child(label.clone()).into_any_element()
                };
                let mut label_row = div().h(px(18.)).flex().items_center().gap_2().child(label);
                if let Some(detail) = self
                    .content
                    .step_details
                    .get(index)
                    .and_then(Option::as_ref)
                {
                    label_row = label_row.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail.clone()),
                    );
                }
                let item = StepperItem::new().child(label_row);
                let item = if index + 1 < self.content.steps.len() {
                    item.pb_3()
                } else {
                    item
                };
                if index < active {
                    item.icon(IconName::CircleCheck)
                } else {
                    item
                }
            })
            .collect::<Vec<_>>();

        div()
            .id("busy-overlay")
            .test_support()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().overlay)
            .opacity(presence.progress.clamp(0.0, 1.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .max_w(px(560.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .px_6()
                    .py_5()
                    .child(
                        Spinner::new()
                            .with_size(gpui_kit::component::Size::Medium)
                            .color(cx.theme().primary),
                    )
                    .child(
                        div()
                            .id("busy-overlay-label")
                            .test_support()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(self.content.label.clone().unwrap_or_default()),
                    )
                    .when(!steps.is_empty(), |card| {
                        card.child(
                            Stepper::new("busy-overlay-steps")
                                .vertical()
                                .small()
                                .disabled(true)
                                .selected_index(active)
                                .items(steps),
                        )
                    })
                    .when_some(self.content.hint.clone(), |card, hint| {
                        card.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(hint),
                        )
                    })
                    .when_some(self.content.cancel_label.clone(), |card, label| {
                        card.child(
                            Button::new("busy-overlay-cancel")
                                .secondary()
                                .small()
                                .label(label)
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(BusyOverlayEvent::Cancel);
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync_reset(overlay: &mut BusyOverlay, visible: bool, label: Option<String>) -> bool {
        overlay.update_state(
            visible,
            BusyOverlayContent {
                label,
                ..Default::default()
            },
        )
    }

    #[test]
    fn hiding_after_an_error_keeps_the_last_label_for_the_fade_out() {
        let mut overlay = BusyOverlay::new();
        assert!(sync_reset(
            &mut overlay,
            true,
            Some("正在重置课程".to_string())
        ));
        assert!(overlay.visible);
        assert_eq!(overlay.content.label.as_deref(), Some("正在重置课程"));

        assert!(sync_reset(&mut overlay, false, None));
        assert!(!overlay.visible);
        assert_eq!(overlay.content.label.as_deref(), Some("正在重置课程"));
    }

    #[test]
    fn show_without_a_transition_label_uses_the_default_label() {
        let mut overlay = BusyOverlay::new();
        assert!(sync_reset(&mut overlay, true, None));
        assert_eq!(
            overlay.content.label.as_deref(),
            Some(t("reset.default_label").as_str())
        );
    }

    #[test]
    fn startup_steps_keep_the_active_stage_and_hint() {
        let mut overlay = BusyOverlay::new();
        let steps = vec!["环境".to_string(), "连接".to_string(), "工作区".to_string()];
        assert!(overlay.update_state(
            true,
            BusyOverlayContent {
                label: Some(t("startup.title")),
                steps: steps.clone(),
                active_step: Some(1),
                hint: Some(t("startup.first_run")),
                ..Default::default()
            },
        ));
        assert_eq!(overlay.content.steps, steps);
        assert_eq!(overlay.content.active_step, Some(1));
        assert_eq!(
            overlay.content.hint.as_deref(),
            Some(t("startup.first_run").as_str())
        );

        assert!(overlay.update_state(
            false,
            BusyOverlayContent {
                label: None,
                steps: steps.clone(),
                active_step: Some(1),
                hint: Some(t("startup.first_run")),
                ..Default::default()
            },
        ));
        assert!(!overlay.visible);
        assert_eq!(overlay.content.steps, steps);
    }
}
