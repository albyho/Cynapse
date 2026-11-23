//! Playback planning error paths.

use super::super::*;
use super::support::copied_fixture;

#[test]
fn resume_action_cursor_is_persisted_for_the_current_step() {
    let (directory, package) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch_multi_step/main", true).unwrap();
    let resume = session.playback_resume().expect("resume");
    assert_eq!(resume.action_index, None);

    session
        .record_resume_action(&resume.unit_path, &resume.step_id, 1)
        .unwrap();
    let resume = session.playback_resume().expect("resume");
    assert_eq!(resume.action_index, Some(1));
}

#[test]
fn playback_steps_does_not_hide_preparation_errors() {
    let (directory, package) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch_multi_step/main", true).unwrap();
    assert!(!session.playback_steps().is_empty());

    // Simulate a session whose active course disappeared while a Unit was
    // still selected; preparation must surface this, not become empty steps.
    session.active_course = None;

    assert!(session.playback_steps().is_empty());
    let error = session.try_playback_steps().unwrap_err();
    assert!(error.to_string().contains("active course"), "{error}");
}
