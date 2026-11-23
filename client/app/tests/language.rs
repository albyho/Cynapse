//! Locale switching at the Client App seam.
//!
//! This lives in its own test binary because the locale is process-global.

use cynapse_client::settings;
use cynapse_client::view_model::ClientViewModel;
use cynapse_client::views::main::MainView;
use cynapse_client_core::LanguagePreference;
use cynapse_client_model::{ClientModel, ClientSettings};
use gpui_kit::base::Root;
use gpui_kit::prelude::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Entity, TestAppContext, px, size};
use std::path::PathBuf;

fn open_main_window(
    cx: &mut TestAppContext,
) -> (
    gpui_kit::WindowHandle<Root>,
    Entity<ClientViewModel>,
    tempfile::TempDir,
) {
    cx.update(gpui_kit::init);
    let directory = tempfile::tempdir().expect("temp dir");
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (mut model, events) =
        ClientModel::new_at(ClientSettings::default(), directory.path().join("state.db"))
            .expect("test model");
    // Same reason as the ui tests: no background adapter, hand-injected events.
    model.session().suppress_local_adapter();
    let state = futures::executor::block_on(model.session().open_course(package))
        .expect("open test course");
    model.apply_course_state(state);

    let view_model_handle = std::rc::Rc::new(std::cell::RefCell::new(None));
    let view_model_for_window = view_model_handle.clone();
    let window = cx.open_window(size(px(640.), px(900.)), move |window, cx| {
        let view_model = cx.new(|cx| ClientViewModel::new_without_narration(model, events, cx));
        *view_model_for_window.borrow_mut() = Some(view_model.clone());
        let main = cx.new(|cx| MainView::new(view_model, window, cx));
        Root::new(main, window, cx)
    });
    let view_model = view_model_handle
        .borrow()
        .clone()
        .expect("view model created");
    (window, view_model, directory)
}

#[gpui_kit::test]
fn switching_language_updates_labels_and_settings(cx: &mut TestAppContext) {
    let (window, view_model, _directory) = open_main_window(cx);

    settings::apply_locale(settings::resolve_locale(LanguagePreference::En));
    assert_eq!(
        cynapse_client::platform::tray::menu_labels(),
        ("Settings".to_string(), "Quit".to_string())
    );
    assert_eq!(cynapse_client_i18n::t("settings.page.general"), "General");
    assert_eq!(cynapse_client_i18n::t("settings.appearance.theme"), "Theme");
    assert_eq!(cynapse_client_i18n::t("settings.captions.font"), "Font");
    assert_eq!(
        cynapse_client_i18n::t("window.settings.title"),
        "Settings - Cynapse Client"
    );
    assert_eq!(
        cynapse_client_i18n::t("error.adapter_disconnected"),
        "The required target software is not connected. Connect it and start learning again."
    );

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("start-learning").label(), Some("Start"));

        settings::apply_locale(settings::resolve_locale(LanguagePreference::ZhCn));
        view_model.update(cx, |view_model, cx| {
            view_model.set_language(LanguagePreference::ZhCn, cx)
        });
        window.render_frame(cx);

        assert_eq!(window.find("start-learning").label(), Some("开始"));
        assert_eq!(
            view_model.read(cx).settings().language,
            LanguagePreference::ZhCn
        );
        assert_eq!(
            cynapse_client::platform::tray::menu_labels(),
            ("设置".to_string(), "退出".to_string())
        );
        assert_eq!(cynapse_client_i18n::t("settings.page.general"), "通用");
        assert_eq!(cynapse_client_i18n::t("settings.appearance.theme"), "主题");
        assert_eq!(cynapse_client_i18n::t("settings.captions.font"), "字体");
        assert_eq!(
            cynapse_client_i18n::t("window.settings.title"),
            "设置 - Cynapse Client"
        );
        assert_eq!(
            cynapse_client_i18n::t("error.adapter_disconnected"),
            "所需目标软件未连接。连接后重新开始学习。"
        );
    })
    .expect("main window");

    settings::apply_locale(settings::resolve_locale(LanguagePreference::En));
}
