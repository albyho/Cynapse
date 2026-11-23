//! Warning identity and rendering.
//!
//! `AdapterDisconnected` keeps its identity and produces text at render time,
//! so a warning pushed under one locale is shown in whichever locale is active
//! when it is displayed. `Message` keeps the text it was created with.

use super::*;

/// The i18n locale is process-global; the one test that switches it runs under
/// this lock so it cannot observe another test's locale.
static LOCALE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn adapter_warning_text_follows_the_active_locale() {
    let guard = LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    cynapse_client_i18n::set_locale("en");
    let english = Warning::AdapterDisconnected.text();

    cynapse_client_i18n::set_locale("zh-CN");
    let chinese = Warning::AdapterDisconnected.text();
    cynapse_client_i18n::set_locale("en");
    drop(guard);

    assert_eq!(
        english,
        "The required target software is not connected. Connect it and start learning again."
    );
    assert_eq!(chinese, "所需目标软件未连接。连接后重新开始学习。");
    assert_eq!(english, adapter_disconnected_message());
    assert_eq!(
        Warning::AdapterDisconnected.to_string(),
        english,
        "Display renders the same text as `text()`"
    );
}

#[test]
fn a_message_keeps_its_own_text_and_no_identity() {
    let warning: Warning = "boom".to_string().into();

    assert_eq!(warning.text(), "boom");
    assert!(!warning.is_adapter_disconnected());
    assert_eq!(Warning::from("boom"), warning);
}

#[test]
fn the_reducer_compares_warnings_by_value() {
    let mut reducer = ClientStateReducer::new(
        course_state(),
        LearningView::default(),
        PlaybackState::Idle,
        ConnectionState::WaitingForAdapter,
    );

    assert!(reducer.apply(&[ClientEvent::Warnings(vec![Warning::AdapterDisconnected])]));
    assert!(!reducer.apply(&[ClientEvent::Warnings(vec![Warning::AdapterDisconnected])]));
    assert!(reducer.apply(&[ClientEvent::Warnings(vec![
        Warning::AdapterDisconnected,
        Warning::Message("note".to_string()),
    ])]));
}
