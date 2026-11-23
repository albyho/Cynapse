use super::color::parse_hex_color;
use crate::view_model::ClientViewModel;
use cynapse_client_core::{
    CaptionFontFamily, CaptionPosition, DEFAULT_CAPTION_BACKGROUND_COLOR, LanguagePreference,
};
use cynapse_client_i18n::{t, t_with};
use cynapse_client_model::ThemePreference;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState, ColorSelect};
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::*;

struct ColorFieldState {
    picker: Entity<ColorPickerState>,
    _subscription: Subscription,
}

struct SliderFieldState {
    slider: Entity<SliderState>,
    _subscription: Subscription,
}

/// Declarative settings window. Changes apply immediately and are persisted by
/// the app-level intent handler.
pub struct SettingsView {
    view_model: Entity<ClientViewModel>,
    _subscription: Subscription,
}

impl SettingsView {
    pub fn new(view_model: Entity<ClientViewModel>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&view_model, |_, _, cx| cx.notify());
        Self {
            view_model,
            _subscription: subscription,
        }
    }
}

fn color_to_hex(color: Hsla) -> String {
    let rgba = color.to_rgb();
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.r * 255.0).round() as u8,
        (rgba.g * 255.0).round() as u8,
        (rgba.b * 255.0).round() as u8
    )
}

fn color_item(
    key: &'static str,
    title: String,
    description: String,
    initial: String,
    view_model: Entity<ClientViewModel>,
    set_color: fn(&mut ClientViewModel, String, &mut Context<ClientViewModel>),
) -> SettingItem {
    let accessibility_title = title.clone();
    let field = SettingField::render(move |options, window, cx| {
        let initial = initial.clone();
        let view_model = view_model.clone();
        let title = accessibility_title.clone();
        let state = window.use_keyed_state(key, cx, move |window, cx| {
            let picker = cx.new(|cx| {
                ColorPickerState::new(window, cx).default_value(
                    parse_hex_color(&initial).unwrap_or_else(|| rgb(0xffffff).into()),
                )
            });
            let subscription = cx.subscribe_in(
                &picker,
                window,
                move |_, _, event: &ColorPickerEvent, _, cx| {
                    if let ColorPickerEvent::Change(Some(color)) = event {
                        let hex = color_to_hex(*color);
                        view_model.update(cx, |view_model, cx| set_color(view_model, hex, cx));
                    }
                },
            );
            ColorFieldState {
                picker,
                _subscription: subscription,
            }
        });
        let picker = state.read(cx).picker.clone();
        ColorSelect::new(&picker)
            .with_size(options.size())
            .accessibility_label(title)
            .into_any_element()
    });
    SettingItem::new(title, field).description(description)
}

#[allow(clippy::too_many_arguments, clippy::let_and_return)]
fn stepper_field(
    key: &'static str,
    min: f64,
    max: f64,
    step: f64,
    unit: &'static str,
    view_model: Entity<ClientViewModel>,
    value: fn(&ClientViewModel) -> f64,
    set_value: fn(&mut ClientViewModel, f64, &mut Context<ClientViewModel>),
) -> SettingField<SharedString> {
    let field = SettingField::render(move |_options, _, cx| {
        let read_model = view_model.clone();
        let current = value(read_model.read(cx));
        let decrease_model = read_model.clone();
        let increase_model = read_model.clone();
        let decrease_write_model = view_model.clone();
        let increase_write_model = view_model.clone();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Button::new(format!("{key}-decrease"))
                    .label("−")
                    .compact()
                    .disabled(current <= min)
                    .on_click(move |_, _, cx| {
                        let current = value(decrease_model.read(cx));
                        let next = (current - step).max(min).min(max);
                        decrease_write_model
                            .update(cx, |view_model, cx| set_value(view_model, next, cx));
                    }),
            )
            .child(
                div()
                    .w(px(56.))
                    .text_center()
                    .text_sm()
                    .child(format!("{}{unit}", current.round() as i64)),
            )
            .child(
                Button::new(format!("{key}-increase"))
                    .label("+")
                    .compact()
                    .disabled(current >= max)
                    .on_click(move |_, _, cx| {
                        let current = value(increase_model.read(cx));
                        let next = (current + step).min(max).max(min);
                        increase_write_model
                            .update(cx, |view_model, cx| set_value(view_model, next, cx));
                    }),
            )
            .into_any_element()
    });
    field
}

fn narration_volume_field(
    title: String,
    initial: u8,
    view_model: Entity<ClientViewModel>,
) -> SettingField<SharedString> {
    SettingField::render(move |_options, window, cx| {
        let title = title.clone();
        let volume = view_model.read(cx).settings().volume;
        let read_model = view_model.clone();
        let state = window.use_keyed_state("narration-volume", cx, move |window, cx| {
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max(100.0)
                    .step(1.0)
                    .default_value(f32::from(initial))
            });
            let subscription =
                cx.subscribe_in(&slider, window, move |_, _, event: &SliderEvent, _, cx| {
                    let value = match event {
                        SliderEvent::Change(value) | SliderEvent::Release(value) => value,
                    };
                    let volume = value.end().round() as u8;
                    read_model.update(cx, |view_model, cx| view_model.set_volume(volume, cx));
                });
            SliderFieldState {
                slider,
                _subscription: subscription,
            }
        });
        let slider = state.read(cx).slider.clone();
        let icon = match volume {
            0 => IconName::VolumeX,
            1..=49 => IconName::Volume1,
            _ => IconName::Volume2,
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(Icon::new(icon).small())
            .child(
                div()
                    // Three digits ("100") must fit without shifting the slider.
                    .w(px(32.))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(volume.to_string()),
            )
            .child(Slider::new(&slider).w(px(220.)).accessibility_label(title))
            .into_any_element()
    })
}

fn course_store_dir_field(view_model: Entity<ClientViewModel>) -> SettingField<SharedString> {
    SettingField::render(move |_options, _window, cx| {
        let settings = view_model.read(cx).settings();
        let path = settings.resolved_course_store_dir();
        let path_label = path.display().to_string();
        let tooltip_label = path_label.clone();
        let using_default = settings.valid_course_store_dir().is_none();
        let invalid = settings.has_invalid_course_store_dir();
        let has_override = settings.course_store_dir.is_some();
        let path_color = if invalid {
            cx.theme().danger
        } else if using_default {
            cx.theme().muted_foreground
        } else {
            cx.theme().foreground
        };
        let choose_model = view_model.clone();
        let reset_model = view_model.clone();
        let choose_label = t("settings.general.course_store_dir.choose");
        let reset_label = t("settings.general.course_store_dir.reset");
        let default_label = t("settings.general.course_store_dir.default");
        let prompt_label = t("settings.general.course_store_dir.prompt");

        let mut content = div().w_full().flex().flex_col().items_end().gap_1().child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("course-store-dir-path")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(path_color)
                        .tooltip(move |window, cx| {
                            Tooltip::new(tooltip_label.clone()).build(window, cx)
                        })
                        .child(path_label),
                )
                .when(using_default, |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(default_label),
                    )
                })
                .child(
                    Button::new("course-store-dir-choose")
                        .small()
                        .label(choose_label)
                        .on_click(move |_, _, cx| {
                            let paths = cx.prompt_for_paths(PathPromptOptions {
                                files: false,
                                directories: true,
                                multiple: false,
                                prompt: Some(prompt_label.clone().into()),
                            });
                            let model = choose_model.clone();
                            cx.spawn(async move |cx| {
                                if let Ok(Ok(Some(paths))) = paths.await
                                    && let Some(path) =
                                        paths.into_iter().next().filter(|path| path.is_absolute())
                                {
                                    model.update(cx, |model, cx| {
                                        model.set_course_store_dir(Some(path), cx)
                                    });
                                }
                            })
                            .detach();
                        }),
                )
                .when(has_override, |row| {
                    row.child(
                        Button::new("course-store-dir-reset")
                            .ghost()
                            .small()
                            .label(reset_label)
                            .on_click(move |_, _, cx| {
                                reset_model
                                    .update(cx, |model, cx| model.set_course_store_dir(None, cx));
                            }),
                    )
                }),
        );

        if invalid {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(t("settings.general.course_store_dir.invalid")),
            );
        }

        content.into_any_element()
    })
}

/// The variants of a setting, in menu order. The first one is the enum's
/// `#[default]`, which is what an unknown stored value resolves to.
const THEMES: &[ThemePreference] = &[
    ThemePreference::System,
    ThemePreference::Light,
    ThemePreference::Dark,
];

const FONTS: &[CaptionFontFamily] = &[
    CaptionFontFamily::System,
    CaptionFontFamily::MicrosoftYaHei,
    CaptionFontFamily::SimHei,
    CaptionFontFamily::SimSun,
    CaptionFontFamily::DengXian,
];

const LANGUAGES: &[LanguagePreference] = &[
    LanguagePreference::System,
    LanguagePreference::En,
    LanguagePreference::ZhCn,
];

/// The `(value, label)` pairs a dropdown wants. The value is what
/// `SettingField::dropdown` compares against to mark the current choice, so it
/// must never be the label: a localized label stops matching after a language
/// switch and the menu would show nothing as selected.
fn options<T: Copy>(
    items: &[T],
    value: fn(T) -> &'static str,
    label: fn(T) -> String,
) -> Vec<(SharedString, SharedString)> {
    items
        .iter()
        .map(|item| (value(*item).into(), label(*item).into()))
        .collect()
}

fn option_from_value<T: Copy>(items: &[T], value: fn(T) -> &'static str, stored: &str) -> T {
    items
        .iter()
        .copied()
        .find(|item| value(*item) == stored)
        .unwrap_or(items[0])
}

fn theme_value(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::System => "system",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

fn font_value(font: CaptionFontFamily) -> &'static str {
    match font {
        CaptionFontFamily::System => "system",
        CaptionFontFamily::MicrosoftYaHei => "microsoft_yahei",
        CaptionFontFamily::SimHei => "sim_hei",
        CaptionFontFamily::SimSun => "sim_sun",
        CaptionFontFamily::DengXian => "deng_xian",
    }
}

fn language_value(language: LanguagePreference) -> &'static str {
    match language {
        LanguagePreference::System => "system",
        LanguagePreference::ZhCn => "zh_cn",
        LanguagePreference::En => "en",
    }
}

fn theme_label(theme: ThemePreference) -> String {
    match theme {
        ThemePreference::System => t("settings.appearance.theme.system"),
        ThemePreference::Light => t("settings.appearance.theme.light"),
        ThemePreference::Dark => t("settings.appearance.theme.dark"),
    }
}

fn font_label(font: CaptionFontFamily) -> String {
    match font {
        CaptionFontFamily::System => t("settings.captions.font.system"),
        CaptionFontFamily::MicrosoftYaHei => t("settings.captions.font.microsoft_yahei"),
        CaptionFontFamily::SimHei => t("settings.captions.font.sim_hei"),
        CaptionFontFamily::SimSun => t("settings.captions.font.sim_sun"),
        CaptionFontFamily::DengXian => t("settings.captions.font.deng_xian"),
    }
}

fn language_label(language: LanguagePreference) -> String {
    match language {
        LanguagePreference::System => t("settings.general.language.option.system"),
        LanguagePreference::ZhCn => t("settings.general.language.option.zh_cn"),
        LanguagePreference::En => t("settings.general.language.option.english"),
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings_error = self.view_model.read(cx).settings_error().map(str::to_owned);
        let language_field = {
            let read_model = self.view_model.clone();
            let write_model = self.view_model.clone();
            SettingField::dropdown(
                options(LANGUAGES, language_value, language_label),
                move |cx| language_value(read_model.read(cx).settings().language).into(),
                move |value: SharedString, cx| {
                    let language = option_from_value(LANGUAGES, language_value, &value);
                    write_model.update(cx, |view_model, cx| view_model.set_language(language, cx));
                },
            )
        };

        let theme_field = {
            let read_model = self.view_model.clone();
            let write_model = self.view_model.clone();
            SettingField::dropdown(
                options(THEMES, theme_value, theme_label),
                move |cx| theme_value(read_model.read(cx).settings().theme).into(),
                move |value: SharedString, cx| {
                    let theme = option_from_value(THEMES, theme_value, &value);
                    write_model.update(cx, |view_model, cx| view_model.set_theme(theme, cx));
                },
            )
        };

        let font_field = {
            let read_model = self.view_model.clone();
            let write_model = self.view_model.clone();
            SettingField::dropdown(
                options(FONTS, font_value, font_label),
                move |cx| font_value(read_model.read(cx).settings().caption.font_family).into(),
                move |value: SharedString, cx| {
                    let font = option_from_value(FONTS, font_value, &value);
                    write_model.update(cx, |view_model, cx| {
                        view_model.set_caption_font_family(font, cx)
                    });
                },
            )
        };

        let font_size_field = stepper_field(
            "caption-font-size",
            20.0,
            48.0,
            1.0,
            " px",
            self.view_model.clone(),
            |view_model| f64::from(view_model.settings().caption.font_size),
            |view_model, value, cx| view_model.set_caption_font_size(value.round() as u16, cx),
        );

        let opacity_field = stepper_field(
            "caption-background-opacity",
            0.0,
            100.0,
            5.0,
            " %",
            self.view_model.clone(),
            |view_model| f64::from(view_model.settings().caption.background_opacity),
            |view_model, value, cx| view_model.set_caption_opacity(value.round() as u8, cx),
        );

        let text_opacity_field = stepper_field(
            "caption-text-opacity",
            0.0,
            100.0,
            5.0,
            " %",
            self.view_model.clone(),
            |view_model| f64::from(view_model.settings().caption.text_opacity),
            |view_model, value, cx| view_model.set_caption_text_opacity(value.round() as u8, cx),
        );

        let width_field = stepper_field(
            "caption-width",
            50.0,
            100.0,
            5.0,
            " %",
            self.view_model.clone(),
            |view_model| f64::from(view_model.settings().caption.width_percent),
            |view_model, value, cx| view_model.set_caption_width(value.round() as u8, cx),
        );

        let volume_field = narration_volume_field(
            t("settings.sound.volume"),
            self.view_model.read(cx).settings().volume,
            self.view_model.clone(),
        );
        let course_store_field = course_store_dir_field(self.view_model.clone());

        let position_field = {
            let read_model = self.view_model.clone();
            let write_model = self.view_model.clone();
            SettingField::switch(
                move |cx| read_model.read(cx).settings().caption.position == CaptionPosition::Top,
                move |top: bool, cx| {
                    let position = if top {
                        CaptionPosition::Top
                    } else {
                        CaptionPosition::Bottom
                    };
                    write_model.update(cx, |view_model, cx| {
                        view_model.set_caption_position(position, cx)
                    });
                },
            )
        };

        let (text_color, accent_color, background_color) = {
            let settings = self.view_model.read(cx).settings();
            (
                settings.caption.text_color.clone(),
                settings.caption.accent_color.clone(),
                settings.caption.background_color.clone(),
            )
        };
        let background_enabled_field = {
            let read_model = self.view_model.clone();
            let write_model = self.view_model.clone();
            SettingField::switch(
                move |cx| read_model.read(cx).caption_background_enabled(),
                move |enabled, cx| {
                    write_model.update(cx, |view_model, cx| {
                        view_model.set_caption_background_enabled(enabled, cx)
                    });
                },
            )
        };
        let background_color_item = color_item(
            "caption-background-color",
            t("settings.captions.background_color"),
            t("settings.captions.background_color.description"),
            background_color.unwrap_or_else(|| DEFAULT_CAPTION_BACKGROUND_COLOR.into()),
            self.view_model.clone(),
            ClientViewModel::set_caption_background_color,
        );
        let text_color_item = color_item(
            "caption-text-color",
            t("settings.captions.text_color"),
            t("settings.captions.text_color.description"),
            text_color,
            self.view_model.clone(),
            ClientViewModel::set_caption_text_color,
        );
        let accent_color_item = color_item(
            "caption-accent-color",
            t("settings.captions.accent_color"),
            t("settings.captions.accent_color.description"),
            accent_color,
            self.view_model.clone(),
            ClientViewModel::set_caption_accent_color,
        );

        let pages = vec![
            SettingPage::new(t("settings.page.general"))
                .icon(IconName::Settings)
                .description(t("settings.page.general.description"))
                .resettable(false)
                .group(
                    SettingGroup::new().item(
                        SettingItem::new(t("settings.general.language"), language_field)
                            .description(t("settings.general.language.description")),
                    ),
                )
                .group(
                    SettingGroup::new().item(
                        SettingItem::new(
                            t("settings.general.course_store_dir"),
                            course_store_field,
                        )
                        .description(t("settings.general.course_store_dir.description"))
                        .layout(gpui::Axis::Vertical),
                    ),
                ),
            SettingPage::new(t("settings.page.appearance"))
                .icon(IconName::Palette)
                .description(t("settings.page.appearance.description"))
                .resettable(false)
                .group(
                    SettingGroup::new().item(
                        SettingItem::new(t("settings.appearance.theme"), theme_field)
                            .description(t("settings.appearance.theme.description")),
                    ),
                ),
            SettingPage::new(t("settings.page.captions"))
                .icon(IconName::FileText)
                .description(t("settings.page.captions.description"))
                .resettable(false)
                .group(
                    SettingGroup::new()
                        .title(t("settings.captions.group.text"))
                        .items(vec![
                            SettingItem::new(t("settings.captions.font"), font_field)
                                .description(t("settings.captions.font.description")),
                            SettingItem::new(t("settings.captions.font_size"), font_size_field)
                                .description(t("settings.captions.font_size.description")),
                            text_color_item,
                            accent_color_item,
                            SettingItem::new(
                                t("settings.captions.text_opacity"),
                                text_opacity_field,
                            )
                            .description(t("settings.captions.text_opacity.description")),
                        ]),
                )
                .group(
                    SettingGroup::new()
                        .title(t("settings.captions.group.background"))
                        .items(vec![
                            SettingItem::new(
                                t("settings.captions.background_enabled"),
                                background_enabled_field,
                            )
                            .description(t("settings.captions.background_enabled.description")),
                            background_color_item,
                            SettingItem::new(
                                t("settings.captions.background_opacity"),
                                opacity_field,
                            )
                            .description(t("settings.captions.background_opacity.description")),
                        ]),
                )
                .group(
                    SettingGroup::new()
                        .title(t("settings.captions.group.position"))
                        .items(vec![
                            SettingItem::new(t("settings.captions.position"), position_field)
                                .description(t("settings.captions.position.description")),
                            SettingItem::new(t("settings.captions.width"), width_field)
                                .description(t("settings.captions.width.description")),
                        ]),
                ),
            SettingPage::new(t("settings.page.sound"))
                .icon(IconName::Volume2)
                .description(t("settings.page.sound.description"))
                .resettable(false)
                .group(
                    SettingGroup::new().item(
                        SettingItem::new(t("settings.sound.volume"), volume_field)
                            .description(t("settings.sound.volume.description")),
                    ),
                ),
        ];

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::NORMAL)
                        .child(t("window.settings.title")),
                ),
            )
            .when_some(settings_error, |root, error| {
                root.child(
                    div()
                        .px_4()
                        .py_2()
                        .bg(cx.theme().danger.opacity(0.12))
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(t_with("settings.save_failed", &[("error", &error)])),
                )
            })
            .child(
                div().flex_1().min_h_0().child(
                    Settings::new("client-settings")
                        .sidebar_width(px(200.))
                        .sidebar_size_range(px(160.)..px(240.))
                        .with_group_variant(GroupBoxVariant::Outline)
                        .pages(pages),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FONTS, LANGUAGES, THEMES, font_value, language_value, option_from_value, theme_value,
    };
    use cynapse_client_core::{CaptionFontFamily, LanguagePreference};
    use cynapse_client_model::ThemePreference;

    #[test]
    fn dropdown_values_round_trip_without_using_display_copy() {
        for language in [
            LanguagePreference::System,
            LanguagePreference::En,
            LanguagePreference::ZhCn,
        ] {
            assert_eq!(
                option_from_value(LANGUAGES, language_value, language_value(language)),
                language,
                "language value"
            );
        }
        for theme in [
            ThemePreference::System,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            assert_eq!(
                option_from_value(THEMES, theme_value, theme_value(theme)),
                theme,
                "theme value"
            );
        }
        for font in [
            CaptionFontFamily::System,
            CaptionFontFamily::MicrosoftYaHei,
            CaptionFontFamily::SimHei,
            CaptionFontFamily::SimSun,
            CaptionFontFamily::DengXian,
        ] {
            assert_eq!(
                option_from_value(FONTS, font_value, font_value(font)),
                font,
                "font value"
            );
        }
    }

    /// Both helpers fall back to the first row, which the enums declare as
    /// their `#[default]`. Reordering a table must not silently change what an
    /// unknown stored value resolves to.
    #[test]
    fn every_table_starts_with_the_enum_default() {
        assert_eq!(THEMES[0], ThemePreference::default());
        assert_eq!(FONTS[0], CaptionFontFamily::default());
        assert_eq!(LANGUAGES[0], LanguagePreference::default());
    }
}
