use super::super::*;
use super::support::{copy_directory, fixture_path};
fn rust_variables_fixture() -> PathBuf {
    fixture_path("rust-variables-demo")
}

fn rust_variables_command_start() -> CommandExecutionStart<'static> {
    CommandExecutionStart {
        unit_path: "ch03_01_variables_and_mutability/main",
        step_id: "intentional_compile_error_3",
        action_index: 0,
        command: "rustc main.rs",
        cwd: ".",
        timeout_ms: 300_000,
    }
}

fn started_rust_variables_session(db_path: &Path) -> (CourseSession, PathBuf) {
    let package = db_path
        .parent()
        .expect("database has a parent")
        .join("course");
    copy_directory(&rust_variables_fixture(), &package);
    let mut session = CourseSession::new(db_path).unwrap();
    session.open_course(&package).unwrap();
    session.start_first_unit(true).unwrap();
    (session, package)
}

#[test]
fn interrupted_command_restores_paused_progress_with_partial_output() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (mut session, _) = started_rust_variables_session(&db_path);

    session
        .record_command_started(rust_variables_command_start())
        .unwrap();
    session
        .record_command_interrupted(CommandInterruption {
            unit_path: "ch03_01_variables_and_mutability/main".to_string(),
            step_id: "intentional_compile_error_3".to_string(),
            action_index: 0,
            reason: PlaybackInterruptionReason::UserStop,
            exit_code: None,
            output: Some("error[E0384]: partial compiler output".to_string()),
            output_truncated: false,
        })
        .unwrap();

    let restored = CourseSession::new(&db_path).unwrap();
    let course = restored.state().active_course.unwrap();
    let recovery = course.recovery.expect("command recovery");
    assert_eq!(recovery.reason, PlaybackInterruptionReason::UserStop);
    assert_eq!(recovery.command, "rustc main.rs");
    assert_eq!(
        recovery.output.as_deref(),
        Some("error[E0384]: partial compiler output")
    );
    assert_eq!(
        restored.playback_resume().unwrap().recovery.unwrap().reason,
        PlaybackInterruptionReason::UserStop
    );
    let mut resumed = restored;
    let resumed_state = resumed.start_first_unit(false).unwrap();
    assert!(resumed_state.active_course.unwrap().has_progress());
}

#[test]
fn running_command_without_an_ending_is_recovered_as_forced_termination() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (mut session, _) = started_rust_variables_session(&db_path);
    session
        .record_command_started(rust_variables_command_start())
        .unwrap();
    drop(session);

    let restored = CourseSession::new(&db_path).unwrap();
    let course = restored.state().active_course.unwrap();
    let recovery = course.recovery.expect("forced recovery");
    assert_eq!(
        recovery.reason,
        PlaybackInterruptionReason::ForcedTermination
    );
}

#[test]
fn switching_courses_preserves_the_interrupted_progress() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (mut session, package) = started_rust_variables_session(&db_path);
    session
        .record_command_started(rust_variables_command_start())
        .unwrap();
    session
        .record_command_interrupted(CommandInterruption {
            unit_path: "ch03_01_variables_and_mutability/main".to_string(),
            step_id: "intentional_compile_error_3".to_string(),
            action_index: 0,
            reason: PlaybackInterruptionReason::CourseSwitch,
            exit_code: None,
            output: Some("partial".to_string()),
            output_truncated: false,
        })
        .unwrap();

    session
        .open_course(fixture_path("multi-step-demo"))
        .unwrap();
    let reopened = session.open_course(package).unwrap();
    let course = reopened.active_course.unwrap();

    assert!(course.has_progress());
    assert_eq!(
        course.recovery.unwrap().reason,
        PlaybackInterruptionReason::CourseSwitch
    );
}

#[test]
fn persisted_command_output_keeps_a_bounded_truncated_tail() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (mut session, _) = started_rust_variables_session(&db_path);
    let output = format!("HEAD\n{}\nTAIL", "x".repeat(10 * 1024 * 1024));
    session
        .record_command_started(rust_variables_command_start())
        .unwrap();
    session
        .record_command_interrupted(CommandInterruption {
            unit_path: "ch03_01_variables_and_mutability/main".to_string(),
            step_id: "intentional_compile_error_3".to_string(),
            action_index: 0,
            reason: PlaybackInterruptionReason::Timeout,
            exit_code: None,
            output: Some(output),
            output_truncated: false,
        })
        .unwrap();

    let recovery = CourseSession::new(&db_path)
        .unwrap()
        .state()
        .active_course
        .unwrap()
        .recovery
        .unwrap();
    let persisted = recovery.output.unwrap();
    assert!(recovery.output_truncated);
    assert!(persisted.starts_with("[Cynapse: output truncated]\n"));
    assert!(!persisted.contains("HEAD"));
    assert!(persisted.ends_with("TAIL"));
    assert!(persisted.len() <= 10 * 1024 * 1024);
}
