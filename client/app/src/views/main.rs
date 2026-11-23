use crate::settings;
use crate::view_model::{ClientViewModel, NextTarget, NextUnitOrigin, learning_launch_label};
use crate::views::busy_overlay::{BusyOverlay, BusyOverlayContent, BusyOverlayEvent};
use crate::views::challenge_panel::ChallengePanelView;
use crate::views::course_import_dialog::CourseImportDialogView;
use crate::views::course_tree::CourseTreeView;
use cynapse_client_core::{LearningMode, UnitStatus};
use cynapse_client_i18n::{t, t_with};
use cynapse_client_model::{ThemePreference, UnitProgressKind, unit_progress_kind};
use gpui_kit::component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{TestSupportExt as _, *};
use std::time::Duration;

/// Target offset for a breadcrumb click. `max` is the positive scroll range;
/// offsets range from `0` (start) to `-max` (end).
fn breadcrumb_target_offset(offset: Pixels, max: Pixels) -> Pixels {
    if offset <= -max + px(1.) {
        px(0.)
    } else {
        -max
    }
}

/// Declarative main-window view.
///
/// The view holds only the observable [`ClientViewModel`] and a subscription;
/// it renders state and raises user intents. It does not own business state.
pub struct MainView {
    view_model: Entity<ClientViewModel>,
    course_tree: Entity<CourseTreeView>,
    challenge_panel: Entity<ChallengePanelView>,
    busy_overlay: Entity<BusyOverlay>,
    window_handle: gpui_kit::AnyWindowHandle,
    // Give course import dialogs a restore target so closing them doesn't highlight a title-bar button.
    focus_handle: FocusHandle,
    breadcrumb_scroll: ScrollHandle,
    breadcrumb_course: Option<(String, String)>,
    last_course_import_error_id: u64,
    last_course_import_conflict_id: u64,
    _appearance_subscription: Subscription,
    _busy_overlay_subscription: Subscription,
    _subscription: Subscription,
}

impl MainView {
    pub fn new(
        view_model: Entity<ClientViewModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = view_model.read(cx).window_title();
        window.set_window_title(&title);
        let course_tree = cx.new(|cx| CourseTreeView::new(view_model.clone(), cx));
        let challenge_panel = cx.new(|cx| ChallengePanelView::new(view_model.clone(), cx));
        let busy_overlay = cx.new(|_| BusyOverlay::new());
        let busy_overlay_for_subscription = busy_overlay.clone();
        let window_handle = window.window_handle();
        let focus_handle = cx.focus_handle();
        let subscription = cx.observe(&view_model, move |this, view_model, cx| {
            let (
                course_import,
                import_error,
                import_conflict,
                reset_pending,
                stop_pending,
                reset_label,
                startup_visible,
                startup_steps,
                startup_active_step,
            ) = {
                let view_model = view_model.read(cx);
                let startup_visible = view_model.startup_visible();
                let (startup_steps, startup_active_step) = if startup_visible {
                    view_model.startup_steps()
                } else {
                    (Vec::new(), None)
                };
                (
                    view_model.course_import_view(),
                    view_model.course_import_error().cloned(),
                    view_model.course_import_conflict().cloned(),
                    view_model.reset_pending(),
                    view_model.stop_pending(),
                    view_model.transition_label(),
                    startup_visible,
                    startup_steps,
                    startup_active_step,
                )
            };
            let (visible, content) = if let Some(import) = course_import {
                (
                    true,
                    BusyOverlayContent {
                        label: Some(import.label),
                        steps: import.steps,
                        active_step: import.active_step,
                        step_details: import.step_details,
                        hint: None,
                        cancel_label: import.cancellable.then(|| t("course_import.cancel")),
                    },
                )
            } else if startup_visible {
                (
                    true,
                    BusyOverlayContent {
                        label: Some(t("startup.title")),
                        steps: startup_steps,
                        active_step: startup_active_step,
                        hint: Some(t("startup.first_run")),
                        ..Default::default()
                    },
                )
            } else {
                (
                    reset_pending || stop_pending,
                    BusyOverlayContent {
                        label: reset_label,
                        ..Default::default()
                    },
                )
            };
            busy_overlay_for_subscription.update(cx, |overlay, cx| {
                overlay.sync(visible, content, cx);
            });
            if let Some(error) = import_error
                && error.id != this.last_course_import_error_id
            {
                this.last_course_import_error_id = error.id;
                let message = error.message;
                let _ = this.window_handle.update(cx, |_, window, cx| {
                    window.open_alert_dialog(cx, move |alert, _, _| {
                        alert
                            .title(t("course_import.error.title"))
                            .description(message.clone())
                            .width(px(380.))
                    });
                });
            }
            if let Some(conflict) = import_conflict
                && conflict.id != this.last_course_import_conflict_id
            {
                this.last_course_import_conflict_id = conflict.id;
                let view_model = this.view_model.clone();
                let _ = this.window_handle.update(cx, |_, window, cx| {
                    let dialog_view = cx.new(|cx| {
                        CourseImportDialogView::conflict(
                            view_model,
                            conflict.url,
                            conflict.target,
                            window,
                            cx,
                        )
                    });
                    this.focus_handle.focus(window, cx);
                    window.open_dialog(cx, move |dialog, _, _| {
                        dialog
                            .title(t("course_import.conflict.title"))
                            .width(px(400.))
                            .child(dialog_view.clone())
                    });
                });
            }
            cx.notify();
        });
        let busy_overlay_subscription =
            cx.subscribe(&busy_overlay, |this, _, event: &BusyOverlayEvent, cx| {
                if matches!(event, BusyOverlayEvent::Cancel) {
                    this.view_model
                        .update(cx, |view_model, cx| view_model.cancel_course_import(cx));
                }
            });
        let appearance_subscription = cx.observe_window_appearance(window, |this, _, cx| {
            if this.view_model.read(cx).settings().theme == ThemePreference::System {
                settings::apply_theme(ThemePreference::System, cx);
            }
        });

        Self {
            view_model,
            course_tree,
            challenge_panel,
            busy_overlay,
            window_handle,
            focus_handle,
            breadcrumb_scroll: ScrollHandle::new(),
            breadcrumb_course: None,
            last_course_import_error_id: 0,
            last_course_import_conflict_id: 0,
            _appearance_subscription: appearance_subscription,
            _busy_overlay_subscription: busy_overlay_subscription,
            _subscription: subscription,
        }
    }
}

/// One title-bar window control: fixed width, centred icon, hover/active fill.
///
/// `danger` paints the close button in the danger palette; the minimize and
/// maximize buttons use the secondary one.
fn window_control(
    cx: &App,
    id: &'static str,
    area: WindowControlArea,
    icon: IconName,
    danger: bool,
) -> impl IntoElement {
    let mut control = div()
        .id(id)
        .flex()
        .w(px(35.))
        .h_full()
        .justify_center()
        .items_center()
        .text_color(cx.theme().foreground)
        .window_control_area(area)
        .child(Icon::new(icon).small());
    control = if danger {
        control
            .hover(|style| {
                style
                    .bg(cx.theme().danger)
                    .text_color(cx.theme().danger_foreground)
            })
            .active(|style| {
                style
                    .bg(cx.theme().danger_active)
                    .text_color(cx.theme().danger_foreground)
            })
    } else {
        control
            .hover(|style| style.bg(cx.theme().secondary_hover))
            .active(|style| style.bg(cx.theme().secondary_active))
    };
    control
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view_model = self.view_model.read(cx);
        let course_title = view_model
            .course_title()
            .unwrap_or_else(|| t("main.no_course"));
        let course_identity = view_model.course_identity();
        if self.breadcrumb_course != course_identity {
            self.breadcrumb_course = course_identity.clone();
            self.breadcrumb_scroll.set_offset(point(px(0.), px(0.)));
        }
        let has_course = view_model.course_title().is_some();
        let units = view_model.units();
        let next_unit_course = course_identity;
        let next_unit_source = view_model.next_unit_source_path();
        let learning_active = view_model.learning_active();
        let learning_mode_label = view_model.learning_mode_label();
        let playback = view_model.playback_label();
        let transition_blocks_controls = view_model.transition_blocks_controls();
        let transition_label = view_model.transition_label();
        let adapter_status = view_model.adapter_status();
        let warning = view_model
            .warning_label()
            .or_else(|| view_model.learning_error().map(str::to_string));
        let challenge_pending = view_model.challenge_pending();
        let has_challenge_panel = view_model.is_learning()
            && view_model.checkpoint_position_key().is_some()
            && view_model.current_challenge().is_some();
        let challenge_panel = has_challenge_panel.then(|| self.challenge_panel.clone());
        let next_action = view_model.next_action();
        let next_action_visible = view_model.next_action_visible(&next_action);
        let next_action_label = match &next_action.target {
            Some(NextTarget::Step) => view_model.advance_label().unwrap_or_else(|| t("next.step")),
            Some(NextTarget::Unit(_)) => t("next.unit"),
            None => String::new(),
        };
        let start_controls_visible = view_model.start_controls_visible();
        let can_retry = view_model.can_retry();
        let reset_pending = view_model.reset_pending();
        let can_reset_course = view_model.can_reset_course();

        let selected_unit = view_model
            .selected_unit_path()
            .and_then(|path| units.iter().find(|unit| unit.path == path));
        let start_choice = selected_unit.and_then(|unit| learning_launch_label(unit.status));
        let start_label = start_choice.clone().unwrap_or_else(|| t("unit.start"));
        let continuing = matches!(
            selected_unit.map(|unit| unit.status),
            Some(UnitStatus::InProgress | UnitStatus::Paused)
        );
        let start_learning_label = if continuing {
            t("main.continue_learning")
        } else {
            t("main.start_learning")
        };
        let show_start_controls =
            start_controls_visible && (selected_unit.is_none() || start_choice.is_some());
        let pending_next_unit = view_model.pending_next_unit();
        let status_busy = transition_label.is_some() || playback.is_some();
        let status_left = transition_label.unwrap_or_else(|| {
            playback.clone().unwrap_or_else(|| {
                if has_course {
                    t("main.not_started")
                } else {
                    t("main.no_course_open")
                }
            })
        });
        let breadcrumb_titles = view_model.selected_unit_breadcrumb();
        let breadcrumb = breadcrumb_titles.iter().fold(
            Breadcrumb::new().child(BreadcrumbItem::new(course_title.clone())),
            |breadcrumb, title| breadcrumb.child(BreadcrumbItem::new(title.clone())),
        );
        let breadcrumb = if let Some(title) = view_model.current_step_title() {
            breadcrumb.child(BreadcrumbItem::new(title))
        } else {
            breadcrumb
        };

        let unit_progress = view_model.current_unit_progress();
        let challenge_progress = unit_progress
            .filter(|progress| progress.challenges_total > 0)
            .map(|progress| {
                let tooltip = t_with(
                    "main.progress.challenges",
                    &[
                        ("total", &progress.challenges_total.to_string()),
                        ("completed", &progress.challenges_passed.to_string()),
                        ("skipped", &progress.challenges_skipped.to_string()),
                    ],
                );
                let mut label = div()
                    .id(format!(
                        "unit-challenge-progress-{}-{}-{}",
                        progress.challenges_skipped,
                        progress.challenges_passed,
                        progress.challenges_total
                    ))
                    .test_support()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t("main.progress.challenges_label"));
                if progress.challenges_skipped > 0 {
                    label = label
                        .child(
                            div()
                                .text_color(cx.theme().warning)
                                .child(format!("{}", progress.challenges_skipped)),
                        )
                        .child("|");
                } else if progress.challenges_passed < progress.challenges_total {
                    label = label.child("0|");
                }
                label
                    .child(format!(
                        "{}/{}",
                        progress.challenges_passed, progress.challenges_total
                    ))
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            });
        let step_progress = unit_progress
            .filter(|progress| progress.steps_total > 0)
            .map(|progress| {
                let tooltip = if let Some(step) = progress.in_progress_step {
                    t_with(
                        "main.progress.steps",
                        &[
                            ("total", &progress.steps_total.to_string()),
                            ("completed", &progress.steps_completed.to_string()),
                            ("current", &step.to_string()),
                        ],
                    )
                } else {
                    t_with(
                        "main.progress.steps_no_current",
                        &[
                            ("total", &progress.steps_total.to_string()),
                            ("completed", &progress.steps_completed.to_string()),
                        ],
                    )
                };
                let count = progress.steps_completed + progress.steps_in_progress;
                let mut label = div()
                    .id(format!(
                        "unit-step-progress-{}-{}-{}",
                        progress.steps_completed, progress.steps_in_progress, progress.steps_total
                    ))
                    .test_support()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t("main.progress.steps_label"));
                if progress.in_progress_step.is_some() {
                    label = label.child(
                        div()
                            .text_color(cx.theme().warning)
                            .child(format!("{count}")),
                    );
                } else {
                    label = label.child(format!("{count}"));
                }
                label
                    .child(format!("/{}", progress.steps_total))
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            });

        let next_unit_prompt = pending_next_unit.map(|target| {
            let progress_kind = unit_progress_kind(&target);
            let can_continue = progress_kind == UnitProgressKind::Partial;
            let unit_title = target.title;
            let unit_path = target.path;
            let view_model = self.view_model.clone();
            let cancel_model = view_model.clone();
            let continue_model = view_model.clone();
            let reset_model = view_model.clone();
            let next_unit_course = next_unit_course.clone();
            let next_unit_source = next_unit_source.clone();
            let cancel_course = next_unit_course.clone();
            let cancel_source = next_unit_source.clone();
            let cancel_path = unit_path.clone();
            let continue_course = next_unit_course.clone();
            let continue_source = next_unit_source.clone();
            let continue_path = unit_path.clone();
            let reset_course = next_unit_course.clone();
            let reset_source = next_unit_source.clone();
            let reset_path = unit_path.clone();
            let reset_button = Button::new("next-unit-reset")
                .label(t("main.reset_after_learning"))
                .on_click(move |_, _, cx| {
                    let course = reset_course.clone();
                    let source = reset_source.clone();
                    let path = reset_path.clone();
                    reset_model.update(cx, |view_model, cx| {
                        view_model.prepare_next_unit_if_current(course, source, path, true, cx)
                    });
                });
            let reset_button = if can_continue {
                reset_button.outline()
            } else {
                reset_button.primary()
            };
            div()
                .id("next-unit-prompt")
                .test_support()
                .flex()
                .flex_col()
                .gap_2()
                .px_4()
                .py_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(t_with("main.next_unit", &[("title", &unit_title)])),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if can_continue {
                            t("main.next_unit.in_progress")
                        } else {
                            t("main.next_unit.completed")
                        }),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("next-unit-cancel")
                                .label(t("common.cancel"))
                                .on_click(move |_, _, cx| {
                                    let course = cancel_course.clone();
                                    let source = cancel_source.clone();
                                    let path = cancel_path.clone();
                                    cancel_model.update(cx, |view_model, cx| {
                                        view_model.cancel_next_unit(course, source, path, cx)
                                    });
                                }),
                        )
                        .child(reset_button)
                        .when(can_continue, |row| {
                            row.child(
                                Button::new("next-unit-continue")
                                    .primary()
                                    .label(t("main.continue_learning"))
                                    .on_click(move |_, _, cx| {
                                        let course = continue_course.clone();
                                        let source = continue_source.clone();
                                        let path = continue_path.clone();
                                        continue_model.update(cx, |view_model, cx| {
                                            view_model.prepare_next_unit_if_current(
                                                course, source, path, false, cx,
                                            )
                                        });
                                    }),
                            )
                        }),
                )
        });

        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            // Keep the empty toolbar as tall as a row of medium buttons.
            .min_h(px(48.))
            .border_t_1()
            .border_color(cx.theme().border)
            .when(show_start_controls, |row| {
                row.child(
                    Popover::new("start-mode")
                        .anchor(Anchor::BottomLeft)
                        .arrow(true)
                        .trigger(
                            Button::new("start-learning")
                                .primary()
                                .label(start_label)
                                .disabled(selected_unit.is_none() || transition_blocks_controls),
                        )
                        .content({
                            let view_model = self.view_model.clone();
                            move |_state, _window, cx| {
                                let popover = cx.entity();
                                let step_popover = popover.clone();
                                let continuous_popover = popover.clone();
                                let step_model = view_model.clone();
                                let continuous_model = view_model.clone();
                                div()
                                    .w(px(360.))
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(start_learning_label.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t("main.mode.by_step.description")),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t("main.mode.continuous.description")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .justify_start()
                                            .gap_2()
                                            .child(
                                                Button::new("step-start")
                                                    .primary()
                                                    .label(if continuing {
                                                        t("main.mode.continue_by_step")
                                                    } else {
                                                        t("main.mode.by_step")
                                                    })
                                                    .on_click(move |_, window, cx| {
                                                        step_popover.update(cx, |state, cx| {
                                                            state.dismiss(window, cx)
                                                        });
                                                        step_model.update(cx, |view_model, cx| {
                                                            view_model.start_learning(
                                                                LearningMode::ByStep,
                                                                cx,
                                                            )
                                                        });
                                                    }),
                                            )
                                            .child(
                                                Button::new("continuous-start")
                                                    .primary()
                                                    .label(if continuing {
                                                        t("main.mode.continue_continuous")
                                                    } else {
                                                        t("main.mode.continuous")
                                                    })
                                                    .on_click(move |_, window, cx| {
                                                        continuous_popover
                                                            .update(cx, |state, cx| {
                                                                state.dismiss(window, cx)
                                                            });
                                                        continuous_model.update(
                                                            cx,
                                                            |view_model, cx| {
                                                                view_model.start_learning(
                                                                    LearningMode::Continuous,
                                                                    cx,
                                                                )
                                                            },
                                                        );
                                                    }),
                                            ),
                                    )
                            }
                        }),
                )
            })
            .when(learning_active && selected_unit.is_some(), |row| {
                row.child(
                    Button::new("stop-learning")
                        .label(view_model.proxy_stop_label())
                        .disabled(!view_model.stop_control_enabled())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.view_model
                                .update(cx, |view_model, cx| view_model.stop_learning(cx));
                        })),
                )
            })
            .when(can_retry, |row| {
                row.child(
                    Button::new("retry")
                        .primary()
                        .label(t("main.retry"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.view_model
                                .update(cx, |view_model, cx| view_model.retry(cx));
                        })),
                )
            })
            .when(next_action_visible, |row| {
                let target = next_action.target.clone();
                let next_action_course = next_unit_course.clone();
                let next_action_source = next_unit_source.clone();
                row.child(
                    Button::new("next-step")
                        .primary()
                        .label(next_action_label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let target = target.clone();
                            let course = next_action_course.clone();
                            let source = next_action_source.clone();
                            this.view_model.update(cx, |view_model, cx| match target {
                                Some(NextTarget::Step) => view_model.advance_step(cx),
                                Some(NextTarget::Unit(target)) => match target.origin {
                                    NextUnitOrigin::Boundary => view_model.advance_unit(cx),
                                    NextUnitOrigin::Cursor => view_model
                                        .prepare_next_unit_if_current(
                                            course,
                                            source,
                                            target.path,
                                            false,
                                            cx,
                                        ),
                                },
                                None => {}
                            });
                        })),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when_some(challenge_progress, |row, progress| row.child(progress))
                    .when_some(step_progress, |row, progress| row.child(progress))
                    .when_some(learning_mode_label, |row, label| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(label),
                        )
                    }),
            );

        div()
            .id("main-view")
            .role(Role::Group)
            .test_support()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(34.))
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
                    .bg(cx.theme().title_bar)
                    .child(
                        div()
                            .h_full()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .pl_3()
                            .pr_4()
                            .window_control_area(WindowControlArea::Drag)
                            .child(img("cynapse-icon.png").size(px(18.)))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(cx.theme().foreground)
                                    .child(view_model.window_title()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .pr_1()
                            .when(can_reset_course, |row| {
                                row.child(
                                    Button::new("reset-course")
                                        .ghost()
                                        .compact()
                                        .icon(IconName::RotateCw)
                                        .tooltip(t("main.reset_course"))
                                        .accessibility_label(t("main.reset_course"))
                                        .disabled(reset_pending || challenge_pending)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            let Some(identity) =
                                                this.view_model.read(cx).course_identity()
                                            else {
                                                return;
                                            };
                                            let view_model = this.view_model.clone();
                                            window.open_alert_dialog(cx, move |alert, _, _| {
                                                let view_model = view_model.clone();
                                                let identity = identity.clone();
                                                alert
                                                    .title(t("main.reset_course.title"))
                                                    .description(t("main.reset_course.message"))
                                                    .show_cancel(true)
                                                    .on_ok(move |_, _, cx| {
                                                        let identity = identity.clone();
                                                        view_model.update(cx, |view_model, cx| {
                                                            view_model.reset_course_if_current(
                                                                identity, cx,
                                                            )
                                                        });
                                                        true
                                                    })
                                            });
                                        })),
                                )
                            })
                            .when(has_course, |row| {
                                row.child(
                                    Button::new("close-course")
                                        .ghost()
                                        .compact()
                                        .icon(IconName::CircleX)
                                        .tooltip(t("main.close_course"))
                                        .accessibility_label(t("main.close_course"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            let Some(identity) =
                                                this.view_model.read(cx).course_identity()
                                            else {
                                                return;
                                            };
                                            let view_model = this.view_model.clone();
                                            window.open_alert_dialog(cx, move |alert, _, _| {
                                                let view_model = view_model.clone();
                                                let identity = identity.clone();
                                                alert
                                                    .title(t("main.close_course.title"))
                                                    .description(t("main.close_course.message"))
                                                    .show_cancel(true)
                                                    .on_ok(move |_, _, cx| {
                                                        let identity = identity.clone();
                                                        view_model.update(cx, |view_model, cx| {
                                                            view_model.close_course_if_current(
                                                                identity, cx,
                                                            )
                                                        });
                                                        true
                                                    })
                                            });
                                        })),
                                )
                            }),
                    )
                    .child(
                        Button::new("open-course")
                            .ghost()
                            .compact()
                            .icon(IconName::FolderOpen)
                            .tooltip(t("main.open_course.local"))
                            .accessibility_label(t("main.open_course.local"))
                            .on_click(cx.listener(|_this, _, _, cx| {
                                let paths = cx.prompt_for_paths(PathPromptOptions {
                                    files: false,
                                    directories: true,
                                    multiple: false,
                                    prompt: Some(t("main.open_course.prompt").into()),
                                });
                                cx.spawn(async move |this, cx| {
                                    if let Ok(Ok(Some(paths))) = paths.await
                                        && let Some(path) = paths.into_iter().next()
                                    {
                                        let _ = this.update(cx, |this, cx| {
                                            this.view_model.update(cx, |view_model, cx| {
                                                view_model.open_course(path, cx)
                                            });
                                        });
                                    }
                                })
                                .detach();
                            })),
                    )
                    .child(
                        Button::new("open-url-course")
                            .ghost()
                            .compact()
                            .icon(IconName::Globe)
                            .tooltip(t("main.open_course.url"))
                            .accessibility_label(t("main.open_course.url"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let dialog_view = cx.new(|cx| {
                                    CourseImportDialogView::new(this.view_model.clone(), window, cx)
                                });
                                this.focus_handle.focus(window, cx);
                                window.open_dialog(cx, move |dialog, _, _| {
                                    dialog
                                        .title(t("course_import.url.title"))
                                        .width(px(400.))
                                        .child(dialog_view.clone())
                                });
                            })),
                    )
                    .child(window_control(
                        cx,
                        "minimize",
                        WindowControlArea::Min,
                        IconName::WindowMinimize,
                        false,
                    ))
                    .child(window_control(
                        cx,
                        "maximize",
                        WindowControlArea::Max,
                        if window.is_maximized() {
                            IconName::WindowRestore
                        } else {
                            IconName::WindowMaximize
                        },
                        false,
                    ))
                    .child(window_control(
                        cx,
                        "close",
                        WindowControlArea::Close,
                        IconName::WindowClose,
                        true,
                    )),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .id("breadcrumb-scroll")
                            .w_full()
                            .flex()
                            .track_scroll(&self.breadcrumb_scroll)
                            .overflow_x_scroll()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                                    let max = this.breadcrumb_scroll.max_offset().x;
                                    let offset = this.breadcrumb_scroll.offset().x;
                                    let target = breadcrumb_target_offset(offset, max);
                                    this.breadcrumb_scroll.set_offset(point(target, px(0.)));
                                    cx.notify();
                                }),
                            )
                            .child(breadcrumb.flex_none()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .pl_2()
                    .gap_3()
                    .child(div().flex_1().min_h_0().child(self.course_tree.clone())),
            )
            .when_some(challenge_panel, |root, panel| {
                root.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .px_4()
                        .pt_2()
                        .pb_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .overflow_y_scrollbar()
                        .child(panel),
                )
            })
            .when_some(next_unit_prompt, |root, prompt| root.child(prompt))
            .when_some(warning, |root, warning| {
                root.child(
                    div().px_4().py_2().child(
                        div()
                            .id("client-warning")
                            .test_support()
                            .flex()
                            .items_start()
                            .gap_2()
                            .w_full()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().danger.opacity(0.25))
                            .bg(cx.theme().danger.opacity(0.08))
                            .px_3()
                            .py_2()
                            .child(
                                div().flex_none().h(px(20.)).flex().items_center().child(
                                    Icon::new(IconName::TriangleAlert)
                                        .xsmall()
                                        .text_color(cx.theme().danger),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .max_h(px(160.))
                                    .overflow_y_scrollbar()
                                    .text_sm()
                                    .line_height(px(20.))
                                    .text_color(cx.theme().danger)
                                    .child(warning),
                            ),
                    ),
                )
            })
            .child(toolbar)
            .child({
                let color = if adapter_status.connected {
                    cx.theme().success
                } else if adapter_status.required {
                    cx.theme().warning
                } else {
                    cx.theme().muted_foreground
                };
                StatusBar::new()
                    .left(
                        Button::new("target-software-status")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Network)
                            .accessibility_label(adapter_status.tooltip.clone())
                            .tooltip(adapter_status.tooltip)
                            .text_color(color),
                    )
                    .left(
                        div()
                            .id("status-scroll")
                            .flex()
                            .min_w_0()
                            .overflow_x_scroll()
                            .child(div().flex_none().whitespace_nowrap().child(if status_busy {
                                ShimmerText::new(status_left)
                                    .duration(Duration::from_secs(3))
                                    .spread(0.25)
                                    .into_any_element()
                            } else {
                                div().child(status_left).into_any_element()
                            })),
                    )
            })
            .child(div().absolute().inset_0().child(self.busy_overlay.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::breadcrumb_target_offset;
    use gpui_kit::px;

    #[test]
    fn breadcrumb_click_goes_to_the_opposite_end() {
        assert_eq!(breadcrumb_target_offset(px(0.), px(120.)), -px(120.));
        assert_eq!(breadcrumb_target_offset(-px(120.), px(120.)), px(0.));
        assert_eq!(breadcrumb_target_offset(-px(60.), px(120.)), -px(120.));
        assert_eq!(breadcrumb_target_offset(px(0.), px(0.)), px(0.));
    }
}
