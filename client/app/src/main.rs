//! Cynapse Client GPUI shell.
//!
//! Naming: `ClientModel` is the UI-free application model; `ClientViewModel`
//! is the observable GPUI entity; `MainView`/`SettingsView`/`CaptionView` are
//! declarative views; `platform` owns windows and OS effects.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cynapse_client::platform::tray::TrayEvent;
use cynapse_client::view_model::{ClientIntent, ClientViewModel};
use cynapse_client::{assets, platform, settings, view_model};
use cynapse_client_i18n::{t, t_with};
use cynapse_client_model::ClientModel;
use gpui_kit::*;
use std::path::PathBuf;

fn startup_error_message(error: &str, db_path: &std::path::Path) -> String {
    let db_path = db_path.display().to_string();
    t_with("startup.error", &[("db_path", &db_path), ("error", error)])
}

fn startup_course_from(
    args: impl IntoIterator<Item = String>,
    env: Option<&str>,
) -> Option<PathBuf> {
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if argument == "--course" {
            return args
                .next()
                .filter(|value| !value.trim().is_empty() && !value.starts_with("--"))
                .map(PathBuf::from);
        }
        if let Some(value) = argument.strip_prefix("--course=") {
            return (!value.trim().is_empty()).then(|| PathBuf::from(value));
        }
    }
    env.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Places the caption at the persisted edge, flipped while the pointer is over
/// its base band, and records that band for the hover poll.
fn place_caption(
    cx: &mut App,
    view_model: Entity<ClientViewModel>,
    main_window: platform::MainWindow,
    caption_window: platform::CaptionWindow,
) {
    let position = view_model.read(cx).caption_position();
    let width = view_model.read(cx).caption_width_percent();
    let height = view_model.read(cx).caption_height();
    cx.spawn(async move |cx| {
        let rect =
            cx.update(|cx| platform::caption_rect(cx, main_window.handle, position, width, height));
        if let Some(rect) = rect {
            view_model.update(cx, |view_model, _| view_model.set_caption_rect(Some(rect)));
            platform::show_caption_window(caption_window, rect);
        }
    })
    .detach();
}

fn main() {
    let level = match std::env::var("CYNAPSE_LOG").as_deref() {
        Ok("trace") => tracing::Level::TRACE,
        Ok("debug") => tracing::Level::DEBUG,
        _ => tracing::Level::INFO,
    };
    tracing_subscriber::fmt().with_max_level(level).init();

    let startup_course = startup_course_from(
        std::env::args().skip(1),
        std::env::var("CYNAPSE_PACKAGE").ok().as_deref(),
    );

    #[cfg(debug_assertions)]
    {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            eprintln!(
                "PANIC: {info}\n{}",
                std::backtrace::Backtrace::force_capture()
            );
            default_hook(info);
        }));
    }

    let (activation_tx, mut activation_rx) = tokio::sync::mpsc::unbounded_channel();
    let single_instance = match platform::single_instance::acquire(activation_tx) {
        platform::single_instance::Acquire::Primary(single_instance) => single_instance,
        platform::single_instance::Acquire::AlreadyRunning => {
            tracing::info!(target: "cynapse.client", "another Cynapse Client instance is already running");
            return;
        }
        platform::single_instance::Acquire::Failed(error) => {
            tracing::error!(
                target: "cynapse.client",
                %error,
                "could not set up the single-instance guard"
            );
            // This fails before `run` applies the persisted locale, and the
            // prompt is user-facing, so resolve it here.
            let settings = settings::load();
            settings::apply_locale(settings::resolve_locale(settings.language));
            let message = t_with("startup.instance_error", &[("error", &error)]);
            platform::show_error(&message);
            std::process::exit(1);
        }
    };

    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.set_app_identity("com.cynapse.client", "Cynapse Client");
            let settings = settings::load();
            settings::apply_locale(settings::resolve_locale(settings.language));
            settings::apply_theme(settings.theme, cx);
            let (model, events) =
                match ClientModel::new_with_startup_course(settings, startup_course) {
                Ok(model) => model,
                Err(error) => {
                    let db_path = cynapse_client_core::global_state_db_path();
                    let message = startup_error_message(&error, &db_path);
                    tracing::error!(target: "cynapse.client", %error, "could not start the client model");
                    platform::show_error(&message);
                    std::process::exit(1);
                }
            };
            let view_model = cx.new(|cx| view_model::ClientViewModel::new(model, events, cx));
            let main_window = platform::open_main_window(cx, view_model.clone());
            let mut last_learner_wait = view_model.read(cx).learner_wait_key();
            let mut main_window_topmost = last_learner_wait.is_some();
            platform::set_main_window_topmost(main_window, main_window_topmost);
            cx.observe(&view_model, move |view_model, cx| {
                let wait = view_model.read(cx).learner_wait_key();
                let topmost = wait.is_some();
                if topmost && wait != last_learner_wait {
                    platform::show_main_window_no_activate(main_window);
                    platform::set_main_window_topmost(main_window, true);
                } else if topmost != main_window_topmost {
                    platform::set_main_window_topmost(main_window, topmost);
                }
                last_learner_wait = wait;
                main_window_topmost = topmost;
            })
            .detach();
            let caption_window = platform::open_caption_window(cx, view_model.clone());
            let settings_window = platform::open_settings_window(cx, view_model.clone());
            let (tray_tx, mut tray_rx) = tokio::sync::mpsc::unbounded_channel();

            cx.subscribe(
                &view_model,
                move |view_model, intent: &ClientIntent, cx| match intent {
                    ClientIntent::ShowCaption => {
                        let visible = view_model.read(cx).caption_visible();
                        let position = view_model.read(cx).caption_position();
                        let width = view_model.read(cx).caption_width_percent();
                        let height = view_model.read(cx).caption_height();
                        tracing::debug!(
                            target: "cynapse.client.caption",
                            visible,
                            width,
                            height,
                            ?position,
                            "caption show requested"
                        );
                        place_caption(cx, view_model, main_window, caption_window);
                    }
                    ClientIntent::HideCaption => {
                        tracing::debug!(
                            target: "cynapse.client.caption",
                            "caption hide requested"
                        );
                        cx.spawn(async move |_| platform::hide_caption_window(caption_window))
                            .detach();
                    }
                    ClientIntent::CaptionHeight(height) => {
                        let visible = view_model.read(cx).caption_visible();
                        tracing::debug!(
                            target: "cynapse.client.caption",
                            height,
                            visible,
                            "caption height measured"
                        );
                        if visible {
                            place_caption(cx, view_model, main_window, caption_window);
                        }
                    }
                    ClientIntent::CaptionFlipped => {
                        if view_model.read(cx).caption_visible() {
                            place_caption(cx, view_model, main_window, caption_window);
                        }
                    }
                    ClientIntent::SettingsChanged {
                        settings,
                        theme_changed,
                        caption_geometry_changed,
                    } => {
                        let caption_visible = view_model.read(cx).caption_visible();
                        tracing::debug!(
                            target: "cynapse.client.settings",
                            theme_changed = *theme_changed,
                            caption_geometry_changed = *caption_geometry_changed,
                            caption_visible,
                            caption_width = settings.caption.width_percent,
                            caption_height = view_model.read(cx).caption_height(),
                            caption_position = ?settings.caption.position,
                            "client settings changed"
                        );
                        match settings::save(settings) {
                            Ok(()) => view_model.update(cx, |view_model, cx| {
                                view_model.set_settings_error(None, cx)
                            }),
                            Err(error) => {
                                tracing::warn!(
                                    target: "cynapse.client",
                                    %error,
                                    "could not save client settings"
                                );
                                view_model.update(cx, |view_model, cx| {
                                    view_model.set_settings_error(Some(error.to_string()), cx)
                                });
                            }
                        }
                        let locale = settings::resolve_locale(settings.language);
                        if cynapse_client_i18n::locale() != locale {
                            settings::apply_locale(locale);
                            cx.refresh_windows();
                            let title = t("window.settings.title");
                            settings_window
                                .handle
                                .update(cx, |_, window, _| window.set_window_title(&title))
                                .ok();
                            platform::tray::refresh(cx);
                        }
                        if *theme_changed {
                            let theme = settings.theme;
                            cx.spawn(async move |cx| {
                                cx.update(|cx| settings::apply_theme(theme, cx));
                            })
                            .detach();
                        }
                        if *caption_geometry_changed && view_model.read(cx).caption_visible() {
                            place_caption(cx, view_model, main_window, caption_window);
                        }
                    }
                },
            )
            .detach();

            // A second launch activates the existing process.
            cx.spawn(async move |cx| {
                let _guard = single_instance;
                while activation_rx.recv().await.is_some() {
                    platform::show_main_window(main_window);
                    cx.update(|cx| {
                        main_window
                            .handle
                            .update(cx, |_, window, _| window.activate_window())
                            .ok();
                    });
                }
            })
            .detach();

            if let Err(error) = platform::tray::install(tray_tx, cx) {
                let message = t("tray.install_failed");
                tracing::error!(target: "cynapse.client", %error, "{message}");
                platform::show_error(&message);
                cx.quit();
                return;
            }

            cx.spawn(async move |cx| {
                while let Some(event) = tray_rx.recv().await {
                    match event {
                        TrayEvent::Activate => {
                            platform::show_main_window(main_window);
                            cx.update(|cx| {
                                main_window
                                    .handle
                                    .update(cx, |_, window, _| window.activate_window())
                                    .ok();
                            });
                        }
                        TrayEvent::Settings => {
                            platform::show_settings_window(settings_window);
                            cx.update(|cx| {
                                settings_window
                                    .handle
                                    .update(cx, |_, window, _| window.activate_window())
                                    .ok();
                            });
                        }
                        TrayEvent::Quit => {
                            if platform::window::confirm_exit() {
                                cx.update(|cx| {
                                    platform::tray::close(cx);
                                    cx.quit();
                                });
                            }
                        }
                    }
                }
            })
            .detach();

            // Closing the main window hides it to the tray; this fallback only
            // runs when something removes the window programmatically.
            let main_window_id = main_window.handle.window_id();
            cx.on_window_closed(move |cx, closed| {
                if closed == main_window_id {
                    cx.quit();
                }
            })
            .detach();

            let quit_model = view_model.clone();
            cx.on_app_quit(move |cx| {
                if let Some(settings) = quit_model.update(cx, |model, _| model.take_pending_volume_settings())
                    && let Err(error) = settings::save(&settings)
                {
                    tracing::warn!(target: "cynapse.client", %error, "could not save volume while exiting");
                }
                platform::save_main_window_bounds(cx, main_window.handle);
                let session = quit_model.read(cx).session();
                async move {
                    if let Err(error) = session.cancel_for_exit().await {
                        tracing::warn!(
                            target: "cynapse.client",
                            %error,
                            "could not cancel playback while exiting"
                        );
                    }
                }
            })
            .detach();
        });
}

#[cfg(test)]
mod tests {
    use super::{startup_course_from, startup_error_message};
    use std::path::PathBuf;

    #[test]
    fn startup_course_uses_an_argument_before_the_environment() {
        let path = |value: &str| std::iter::once(value.to_string());

        assert_eq!(
            startup_course_from(path("--course=explicit-course"), Some("environment-course")),
            Some(PathBuf::from("explicit-course"))
        );
        assert_eq!(
            startup_course_from(
                ["--course".to_string(), "split-course".to_string()],
                Some("environment-course")
            ),
            Some(PathBuf::from("split-course"))
        );
        assert_eq!(
            startup_course_from(Vec::new(), Some("environment-course")),
            Some(PathBuf::from("environment-course"))
        );
        assert_eq!(
            startup_course_from(Vec::new(), Some("  ")),
            None,
            "an empty environment value must not select a course"
        );
        assert_eq!(
            startup_course_from(["--course".to_string()], Some("environment-course")),
            None,
            "a missing --course value must not fall through to the environment"
        );
    }

    #[test]
    fn startup_error_message_names_the_database_and_recovery_step() {
        cynapse_client_i18n::set_locale("en");
        let message =
            startup_error_message("file is not a database", std::path::Path::new("state.db"));

        assert!(message.contains("state.db"), "{message}");
        assert!(message.contains("file is not a database"), "{message}");
        assert!(message.contains("back it up"), "{message}");
    }
}
