//! URL and overwrite dialog for opening a Course Package from a ZIP URL.

use crate::view_model::ClientViewModel;
use cynapse_client_i18n::{t, t_with};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::{
    DialogClose, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{Focusable as _, *};
use std::path::PathBuf;

struct Conflict {
    url: String,
    target: PathBuf,
}

pub struct CourseImportDialogView {
    view_model: Entity<ClientViewModel>,
    input: Entity<InputState>,
    error: Option<String>,
    conflict: Option<Conflict>,
    focus_pending: bool,
}

impl CourseImportDialogView {
    pub fn new(
        view_model: Entity<ClientViewModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("course_import.url.placeholder")));
        Self {
            view_model,
            input,
            error: None,
            conflict: None,
            focus_pending: true,
        }
    }

    pub fn conflict(
        view_model: Entity<ClientViewModel>,
        url: String,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("course_import.url.placeholder")));
        Self {
            view_model,
            input,
            error: None,
            conflict: Some(Conflict { url, target }),
            focus_pending: false,
        }
    }

    fn submit_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.input.read(cx).value().trim().to_string();
        if url.is_empty() {
            self.error = Some(t("course_import.url.required"));
            cx.notify();
            return;
        }
        match self.view_model.read(cx).course_import_target(&url) {
            Ok(Some(target)) => {
                self.error = None;
                self.conflict = Some(Conflict { url, target });
                cx.notify();
            }
            Ok(None) => {
                window.close_dialog(cx);
                self.view_model.update(cx, |view_model, cx| {
                    view_model.start_course_import(url, None, cx)
                });
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn open_existing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conflict) = self.conflict.take() else {
            return;
        };
        window.close_dialog(cx);
        self.view_model.update(cx, |view_model, cx| {
            view_model.open_course(conflict.target, cx)
        });
    }

    fn overwrite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conflict) = self.conflict.take() else {
            return;
        };
        window.close_dialog(cx);
        self.view_model.update(cx, |view_model, cx| {
            view_model.start_course_import(conflict.url, Some(conflict.target), cx)
        });
    }

    fn render_conflict(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(conflict) = self.conflict.as_ref() else {
            return div().into_any_element();
        };
        let path = conflict.target.display().to_string();
        let active = self
            .view_model
            .read(cx)
            .is_active_course_path(&conflict.target);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .w_full()
            .child(
                DialogHeader::new()
                    .child(DialogTitle::new().child(t("course_import.conflict.title")))
                    .child(
                        DialogDescription::new()
                            .child(div().text_color(cx.theme().foreground).child(t_with(
                                "course_import.conflict.message",
                                &[("path", &path)],
                            )))
                            .when(active, |description| {
                                description.child(
                                    div()
                                        .text_color(cx.theme().warning)
                                        .child(t("course_import.conflict.active_course")),
                                )
                            })
                            .child(t("course_import.conflict.overwrite_warning")),
                    ),
            )
            .child(
                DialogFooter::new()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("course-import-cancel")
                            .secondary()
                            .label(t("course_import.conflict.cancel"))
                            .on_click(cx.listener(|_, _, window, cx| {
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("course-import-open-existing")
                            .label(t("course_import.conflict.open_existing"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_existing(window, cx);
                            })),
                    )
                    .child(
                        Button::new("course-import-overwrite")
                            .danger()
                            .label(t("course_import.conflict.overwrite"))
                            .disabled(active)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.overwrite(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_url(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .w_full()
            .child(
                DialogHeader::new()
                    .child(DialogTitle::new().child(t("course_import.url.title")))
                    .child(DialogDescription::new().child(t("course_import.url.description"))),
            )
            .child(Input::new(&self.input).w_full())
            .when_some(self.error.clone(), |column, error| {
                column.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
            .child(
                DialogFooter::new()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div().flex_1().child(
                            DialogClose::new().child(
                                Button::new("course-import-url-cancel")
                                    .w_full()
                                    .secondary()
                                    .label(t("course_import.conflict.cancel")),
                            ),
                        ),
                    )
                    .child(
                        Button::new("course-import-url-open")
                            .flex_1()
                            .primary()
                            .label(t("course_import.url.open"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_url(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for CourseImportDialogView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_pending {
            self.focus_pending = false;
            self.input.focus_handle(cx).focus(window, cx);
        }
        if self.conflict.is_some() {
            self.render_conflict(cx)
        } else {
            self.render_url(cx)
        }
    }
}
