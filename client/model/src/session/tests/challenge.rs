//! Challenge checkpoint and completion tests.

use super::support::*;
use super::*;
use cynapse_client_core::ChallengeStatus;
use rusqlite::Connection;
use std::time::Duration;

#[test]
fn final_challenge_pass_in_by_step_completes_the_unit_and_ends_learning() {
    final_challenge_ends_learning(LearningMode::ByStep, ChallengeOutcome::Passed);
}

#[test]
fn final_challenge_skip_in_continuous_completes_the_unit_and_ends_learning() {
    final_challenge_ends_learning(LearningMode::Continuous, ChallengeOutcome::Skipped);
}

fn final_challenge_ends_learning(mode: LearningMode, outcome: ChallengeOutcome) {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_final_challenge_course(directory.path());
    let (mut model, mut receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    let unit_path = state
        .selected_unit
        .as_ref()
        .expect("startup unit")
        .path
        .clone();
    model.apply_course_state(state);
    let adapter = test_runtime.spawn(fake_adapter(directory.path().join("runtime.json")));

    test_runtime.block_on(async {
        wait_for_model_connection(&mut model, &mut receiver, true).await;
        model.start_learning(mode, &unit_path).unwrap();
        model
            .session()
            .begin_learning(mode, &unit_path)
            .await
            .unwrap();
        wait_for_model(&mut model, &mut receiver, |model| {
            matches!(
                model.state().playback(),
                PlaybackState::StepComplete {
                    requires_human: true,
                    ..
                }
            )
        })
        .await;

        match outcome {
            ChallengeOutcome::Passed => {
                let result = model
                    .session()
                    .check_challenge(&unit_path, "check", true)
                    .await
                    .unwrap();
                assert_eq!(result.outcome, ChallengeOutcome::Passed);
            }
            ChallengeOutcome::Skipped => {
                let result = model
                    .session()
                    .skip_challenge(&unit_path, "check")
                    .await
                    .unwrap();
                assert_eq!(result.outcome, ChallengeOutcome::Skipped);
            }
            other => panic!("unsupported final Challenge outcome: {other:?}"),
        }

        model.session().advance_step().unwrap();
        wait_for_model(&mut model, &mut receiver, |model| {
            matches!(model.state().playback(), PlaybackState::UnitComplete { .. })
                && !model.is_learning()
                && model
                    .state()
                    .course()
                    .active_course
                    .as_ref()
                    .is_some_and(|course| course.current_unit_path.is_none())
        })
        .await;

        assert!(model.state().course().selected_unit.is_none());
    });

    adapter.abort();
}

#[test]
fn challenge_boundary_does_not_mark_the_step_completed() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let adapter = test_runtime.spawn(fake_adapter(directory.path().join("runtime.json")));

    test_runtime.block_on(async {
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::StepComplete { .. })
        })
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let state = service.course_state().unwrap();
        let course = state.active_course.expect("active course");
        let unit = find_unit(&course.tree, &unit_path).expect("unit");
        let step = unit
            .steps
            .iter()
            .find(|step| step.step_id == "check")
            .expect("challenge step");
        assert!(!step.completed);
        assert_eq!(step.challenge_status, ChallengeStatus::NotStarted);
        let _ = service.stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn persists_terminal_recovery_from_a_paused_playback() {
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(package).unwrap();
    session.start_unit("ch/main", true).unwrap();
    let position = PlaybackPosition {
        unit_path: "ch/main".to_string(),
        step_id: "check".to_string(),
        action_index: 2,
        ..Default::default()
    };

    persist_playback(
        &mut session,
        &PlaybackState::Paused {
            position,
            interruption: PlaybackInterruption {
                reason: cynapse_client_core::PlaybackInterruptionReason::CommandFailure,
                exit_code: Some(1),
                output: Some("not ok".to_string()),
                output_truncated: Some(false),
            },
        },
        &mut None,
    )
    .expect("persist playback");

    let state = session.state();
    let recovery = state
        .active_course
        .expect("active course")
        .recovery
        .expect("command recovery");
    assert_eq!(recovery.command, "node --test");
    assert_eq!(recovery.output.as_deref(), Some("not ok"));
    assert_eq!(recovery.exit_code, Some(1));
}

#[test]
fn reports_a_database_failure_instead_of_ignoring_it() {
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let db_path = directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(package).unwrap();
    session.start_unit("ch/main", true).unwrap();

    // Make the next progress write fail without destroying the open
    // CourseSession connection.
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TRIGGER fail_progress
            BEFORE UPDATE ON course_progress
            BEGIN
                SELECT RAISE(ABORT, 'injected database failure');
            END;
            ",
        )
        .unwrap();
    drop(connection);

    let error = persist_playback(
        &mut session,
        &PlaybackState::Paused {
            position: PlaybackPosition {
                unit_path: "ch/main".to_string(),
                step_id: "check".to_string(),
                action_index: 2,
                ..Default::default()
            },
            interruption: PlaybackInterruption {
                reason: cynapse_client_core::PlaybackInterruptionReason::UserStop,
                exit_code: None,
                output: None,
                output_truncated: None,
            },
        },
        &mut None,
    )
    .expect_err("a failed database write must be returned");

    assert!(
        error.contains("database") || error.contains("SQLite"),
        "unexpected persistence error: {error}"
    );
}
