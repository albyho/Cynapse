//! Challenge panel: instructions, hint, status, check/skip actions, and result.

use crate::view_model::ClientViewModel;
use cynapse_client_core::ChallengeStatus;
use cynapse_client_i18n::{t, t_with};
use gpui_kit::base::{SelectableText, TextSelection};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::text::{MarkdownExtensions, TextView, TextViewStyle};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Duration;

fn text_context_menu(
    menu: PopupMenu,
    all_text: String,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    // The menu item click is a left-button press. The window text-selection
    // handler clears the selection during that press, before this handler runs,
    // so capture the selected text when the menu is built.
    let selected_text = TextSelection::selected_text(window, cx);
    let can_copy_selection = !selected_text.is_empty();
    menu.item(
        PopupMenuItem::new(t("common.copy_selected"))
            .disabled(!can_copy_selection)
            .on_click(move |_, _, cx| {
                if !selected_text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(selected_text.clone()));
                }
            }),
    )
    .item(
        PopupMenuItem::new(t("common.copy_all")).on_click(move |_, _, cx| {
            if !all_text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(all_text.clone()));
            }
        }),
    )
}

/// Renders the active Challenge checkpoint.
///
/// Owns only presentation state (hint expansion); all challenge state and
/// actions stay in [`ClientViewModel`].
pub struct ChallengePanelView {
    view_model: Entity<ClientViewModel>,
    challenge_identity: Option<(String, String)>,
    hint_open: bool,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl ChallengePanelView {
    pub fn new(view_model: Entity<ClientViewModel>, cx: &mut Context<Self>) -> Self {
        let challenge_identity = view_model.read(cx).checkpoint_position_key();
        let subscription = cx.observe(&view_model, |this, view_model, cx| {
            let identity = view_model.read(cx).checkpoint_position_key();
            if this.challenge_identity != identity {
                this.challenge_identity = identity;
                this.hint_open = false;
            }
            cx.notify();
        });
        Self {
            view_model,
            challenge_identity,
            hint_open: false,
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        }
    }
}

impl Render for ChallengePanelView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view_model = self.view_model.read(cx);
        let Some(challenge) = view_model.current_challenge().cloned() else {
            return div().into_any_element();
        };
        let checkpoint = view_model.checkpoint_position_key();
        let finished = matches!(
            view_model.challenge_status(),
            ChallengeStatus::Passed | ChallengeStatus::Skipped
        );
        let hint_open = self.hint_open;
        let status_label = match view_model.challenge_status() {
            ChallengeStatus::NotStarted => t("challenge.status.not_started"),
            ChallengeStatus::Failed => t("challenge.status.failed"),
            ChallengeStatus::Passed => t("challenge.status.passed"),
            ChallengeStatus::Skipped => t("challenge.status.skipped"),
        };
        let attempts = view_model.challenge_attempts();
        let status_text = if attempts == 0 {
            t_with("challenge.status", &[("status", &status_label)])
        } else {
            let attempts = attempts.to_string();
            t_with(
                "challenge.status.attempts",
                &[("status", &status_label), ("attempts", &attempts)],
            )
        };
        let challenge_pending = view_model.challenge_pending();
        let transition_blocks_controls = view_model.transition_blocks_controls();
        let at_checkpoint = view_model.at_checkpoint();
        let adapter_ready = view_model.adapter_ready();
        let challenge_needs_save = view_model.challenge_needs_save();
        let challenge_message = view_model.challenge_message().map(str::to_string);
        let challenge_output = view_model.challenge_output().map(str::to_string);
        let challenge_exit_code = view_model.challenge_exit_code();
        let challenge_output_truncated = view_model.challenge_output_truncated();
        let pending_message = if challenge_pending {
            challenge_message.clone()
        } else {
            None
        };
        let show_result = !challenge_pending
            && (challenge_message.is_some()
                || challenge_output.is_some()
                || challenge_exit_code.is_some()
                || challenge_output_truncated);
        let mut result_copy_text = challenge_message.clone().unwrap_or_default();
        if let Some(output) = &challenge_output {
            if !result_copy_text.is_empty() {
                result_copy_text.push('\n');
            }
            result_copy_text.push_str(output);
        }
        if let Some(exit_code) = challenge_exit_code {
            if !result_copy_text.is_empty() {
                result_copy_text.push('\n');
            }
            result_copy_text.push_str(&t_with(
                "challenge.exit_code",
                &[("code", &exit_code.to_string())],
            ));
        }
        if challenge_output_truncated {
            if !result_copy_text.is_empty() {
                result_copy_text.push('\n');
            }
            result_copy_text.push_str(&t("challenge.output_truncated"));
        }
        let panel_focus = self.focus_handle.clone();

        let panel = GroupBox::new()
            .title(t("challenge.title"))
            .normal()
            .child(div().text_sm().child(challenge.instructions.clone()))
            .when_some(challenge.hint.clone(), |panel, hint| {
                let hint_copy = hint.clone();
                panel.child(
                    Collapsible::new()
                        .open(hint_open)
                        .gap_1()
                        .child(
                            div()
                                .id("challenge-hint")
                                .test_support()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .w_full()
                                .rounded(cx.theme().radius)
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .cursor_pointer()
                                .hover(|style| {
                                    style
                                        .bg(cx.theme().secondary_hover)
                                        .text_color(cx.theme().foreground)
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.hint_open = !this.hint_open;
                                    cx.notify();
                                }))
                                .child(Icon::new(IconName::Info).xsmall())
                                .child(if hint_open {
                                    t("challenge.hide_hint")
                                } else {
                                    t("challenge.show_hint")
                                }),
                        )
                        .content(
                            div()
                                .id("challenge-hint-content")
                                .test_support()
                                .role(Role::Group)
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                                    this.focus_handle.focus(window, cx);
                                }))
                                .w_full()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .p_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    TextView::markdown("challenge-hint-text", hint)
                                        .markdown_extensions(MarkdownExtensions::default().raw_html(false))
                                        .code_block_actions(|block, _, cx| {
                                            let copy_code = t("common.copy_code");
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .children(block.lang())
                                                .child(
                                                    Clipboard::new("challenge-hint-copy-code")
                                                        .value(block.code())
                                                        .tooltip(copy_code.clone())
                                                        .accessibility_label(copy_code),
                                                )
                                        })
                                        .style(
                                            TextViewStyle::default()
                                                .paragraph_gap(rems(0.25))
                                                .code_block(
                                                    StyleRefinement::default()
                                                        .px_2()
                                                        .pt(rems(2.))
                                                        .pb_2()
                                                        .border_color(if cx.theme().is_dark() {
                                                            cx.theme().muted_foreground.opacity(0.6)
                                                        } else {
                                                            cx.theme().border
                                                        }),
                                                ),
                                        )
                                        .selectable(true),
                                )
                                .context_menu(move |menu, window, cx| {
                                    text_context_menu(menu, hint_copy.clone(), window, cx)
                                }),
                        ),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(status_text),
            )
            .when(finished, |panel| {
                let color = if view_model.challenge_status() == ChallengeStatus::Passed {
                    cx.theme().success
                } else {
                    cx.theme().warning
                };
                panel.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .rounded_full()
                                .bg(color.opacity(0.12))
                                .text_sm()
                                .text_color(color)
                                .child(status_label),
                        ),
                )
            })
            .when(!finished, |panel| {
                panel.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("check-challenge")
                                .primary()
                                .label(if challenge_pending {
                                    t("challenge.checking_button")
                                } else if challenge_needs_save {
                                    t("challenge.save_and_check")
                                } else if view_model.challenge_status() == ChallengeStatus::Failed {
                                    t("challenge.check_again")
                                } else {
                                    t("challenge.check")
                                })
                                .disabled(
                                    challenge_pending
                                        || transition_blocks_controls
                                        || !at_checkpoint
                                        || !adapter_ready
                                        || finished,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.view_model.update(cx, |view_model, cx| {
                                        view_model.check_challenge(cx)
                                    });
                                })),
                        )
                        .when(challenge.allow_skip, |row| {
                            let view_model = self.view_model.clone();
                            let checkpoint = checkpoint.clone();
                            row.child(
                                Popover::new("skip-challenge-confirm")
                                    .anchor(Anchor::BottomLeft)
                                    .arrow(true)
                                    .trigger(
                                        Button::new("skip-challenge")
                                            .outline()
                                            .label(t("challenge.skip"))
                                            .disabled(
                                                challenge_pending
                                                    || transition_blocks_controls
                                                    || !at_checkpoint
                                                    || finished,
                                            ),
                                    )
                                    .content(move |_state, _window, cx| {
                                        let popover = cx.entity();
                                        let cancel_popover = popover.clone();
                                        let confirm_popover = popover.clone();
                                        let confirm_model = view_model.clone();
                                        let checkpoint = checkpoint.clone();
                                        div()
                                            .w(px(280.))
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .child(t("challenge.skip.title")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t("challenge.skip.message")),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .justify_start()
                                                    .gap_2()
                                                    .child(
                                                        Button::new("skip-challenge-cancel")
                                                            .label(t("common.cancel"))
                                                            .on_click(move |_, window, cx| {
                                                                cancel_popover.update(
                                                                    cx,
                                                                    |state, cx| {
                                                                        state.dismiss(window, cx)
                                                                    },
                                                                );
                                                            }),
                                                    )
                                                    .child(
                                                        Button::new("skip-challenge-confirm")
                                                            .primary()
                                                            .label(t("challenge.skip"))
                                                            .on_click(move |_, window, cx| {
                                                                confirm_popover.update(
                                                                    cx,
                                                                    |state, cx| {
                                                                        state.dismiss(window, cx)
                                                                    },
                                                                );
                                                                confirm_model.update(
                                                                    cx,
                                                                    |view_model, cx| {
                                                                        if let Some((
                                                                            unit_path,
                                                                            step_id,
                                                                        )) = checkpoint
                                                                            .clone()
                                                                        {
                                                                            view_model
                                                                                .skip_challenge_if_current(
                                                                                    unit_path,
                                                                                    step_id,
                                                                                    cx,
                                                                                );
                                                                        }
                                                                    },
                                                                );
                                                            }),
                                                    ),
                                            )
                                    }),
                            )
                        }),
                )
            })
            .when_some(pending_message, |panel, message| {
                panel.child(
                    ShimmerText::new(message)
                        .duration(Duration::from_secs(3))
                        .spread(0.25)
                        .text_sm(),
                )
            })
            .when(show_result, |panel| {
                panel.child(
                    div()
                        .id("challenge-result")
                        .role(Role::Group)
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                            this.focus_handle.focus(window, cx);
                        }))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .w_full()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted)
                        .p_2()
                        .when_some(challenge_message, |result, message| {
                            result.child(
                                div().text_sm().child(SelectableText::new(
                                    "challenge-result-message",
                                    message,
                                )),
                            )
                        })
                        .when_some(challenge_output, |result, output| {
                            result.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(SelectableText::new(
                                        "challenge-result-output",
                                        output,
                                    )),
                            )
                        })
                        .when_some(challenge_exit_code, |result, exit_code| {
                            result.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(SelectableText::new(
                                        "challenge-result-exit-code",
                                        t_with(
                                            "challenge.exit_code",
                                            &[("code", &exit_code.to_string())],
                                        ),
                                    )),
                            )
                        })
                        .when(challenge_output_truncated, |result| {
                            result.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(SelectableText::new(
                                        "challenge-result-truncated",
                                        t("challenge.output_truncated"),
                                    )),
                            )
                        })
                        .context_menu(move |menu, window, cx| {
                            text_context_menu(menu, result_copy_text.clone(), window, cx)
                        }),
                )
            });

        div()
            .id("challenge-panel")
            .role(Role::Group)
            // Only this node registers panel_focus, so GPUI reports it once to AccessKit.
            .track_focus(&panel_focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if event.keystroke.key.eq_ignore_ascii_case("c")
                    && modifiers.secondary(cx)
                    && !modifiers.alt
                    && !modifiers.shift
                {
                    let text = TextSelection::selected_text(window, cx);
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        cx.stop_propagation();
                    }
                }
            }))
            .child(panel)
            .into_any_element()
    }
}
