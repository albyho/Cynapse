//! Learning start/advance, environment, and runtime tests.

use super::support::*;
use super::*;
use cynapse_client_core::{ChallengeStatus, UnitStatus};
use cynapse_client_i18n::t;
use std::time::Duration;
use tokio::sync::mpsc::{self};
use tokio::time::timeout;

#[test]
fn passes_a_challenge_then_advances_the_playback_boundary() {
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
        let playback = wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::StepComplete { .. })
        })
        .await;
        assert_eq!(
            playback_position(&playback).map(|position| position.step_id.as_str()),
            Some("check")
        );

        let result = service
            .check_challenge(&unit_path, "check", true)
            .await
            .unwrap();
        assert_eq!(result.outcome, ChallengeOutcome::Passed);
        wait_for_challenge_status(&mut receiver, &unit_path, "check", ChallengeStatus::Passed)
            .await;

        service.advance_step().unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        let _ = service.stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn check_timeout_cancels_the_pending_adapter_request() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course_with_timeout(directory.path(), 5);
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let (cancel_tx, mut cancel_rx) = mpsc::unbounded_channel();
    let adapter = test_runtime.spawn(fake_adapter_ignores_check(
        directory.path().join("runtime.json"),
        cancel_tx,
    ));

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

        let error = service
            .check_challenge(&unit_path, "check", true)
            .await
            .expect_err("check should time out");
        assert_eq!(error, t("error.challenge_check_timeout"));
        let target = timeout(Duration::from_secs(2), cancel_rx.recv())
            .await
            .expect("cancel was not sent")
            .expect("cancel channel closed");
        assert!(!target.is_empty());

        let _ = service.stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn skips_a_challenge_then_advances_the_playback_boundary() {
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

        let result = service.skip_challenge(&unit_path, "check").await.unwrap();
        assert_eq!(result.outcome, ChallengeOutcome::Skipped);
        wait_for_challenge_status(&mut receiver, &unit_path, "check", ChallengeStatus::Skipped)
            .await;

        service.advance_step().unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        let _ = service.stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn start_while_disconnected_keeps_learning_idle_and_reports_the_same_problem() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);

    let first = model
        .start_learning(LearningMode::ByStep, "ch_units/first_unit")
        .expect_err("a required Adapter is missing");
    let second = model
        .start_learning(LearningMode::ByStep, "ch_units/first_unit")
        .expect_err("a second Start must report the same problem");
    assert_eq!(first, cynapse_client_core::adapter_disconnected_message());
    assert_eq!(second, first);
    assert_ne!(second, "a learning mode is already active");
    assert_eq!(
        model.state().learning().phase,
        cynapse_client_core::LearningPhase::Idle
    );
    assert_eq!(model.state().learning().mode, None);
}

#[test]
fn course_content_revision_stays_stable_for_progress_and_selection() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/independent-units");
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);
    let content_revision = model.course_content_revision();

    let mut state = model.state().course().clone();
    state.selected_unit = None;
    state.active_course.as_mut().unwrap().current_step_id = Some("step".to_string());
    model.apply_course_state(state);

    assert_eq!(model.course_content_revision(), content_revision);
}

#[test]
fn explicit_selection_never_falls_back_to_the_resume_cursor() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/independent-units");
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);
    assert!(!model.has_progress());

    let mut state = model.state().course().clone();
    let cursor = state
        .active_course
        .as_mut()
        .and_then(|course| course.current_unit_path.clone())
        .or_else(|| state.selected_unit.as_ref().map(|unit| unit.path.clone()))
        .expect("a Unit to resume");
    state.selected_unit = None;
    state.active_course.as_mut().unwrap().current_unit_path = Some(cursor.clone());
    model.apply_course_state(state);

    assert_eq!(
        model.selected_unit_path(),
        None,
        "the resume cursor never makes an unselected Unit appear selected"
    );
    assert!(
        !model.has_progress(),
        "selection and the resume cursor are not progress"
    );
    let course = model
        .state()
        .course()
        .active_course
        .as_ref()
        .expect("active course");
    let unit = find_unit(&course.tree, &cursor).expect("resume cursor Unit");
    assert_eq!(unit.status, UnitStatus::NotStarted);

    let mut state = model.state().course().clone();
    let course = state.active_course.as_ref().unwrap();
    state.selected_unit = Some(find_unit(&course.tree, &cursor).unwrap().clone());
    model.apply_course_state(state);

    assert_eq!(model.selected_unit_path(), Some(cursor.as_str()));
}

#[test]
fn start_does_not_wait_for_an_adapter_that_connects_late() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let runtime_path = directory.path().join("runtime.json");
    let adapter = test_runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        fake_adapter(runtime_path).await;
    });

    test_runtime.block_on(async {
        let error = service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .expect_err("Start must not wait for a late Adapter");
        assert_eq!(error, cynapse_client_core::adapter_disconnected_message());
    });

    adapter.abort();
}

#[test]
fn runtime_failure_keeps_the_active_mode_and_retries_it() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let (mut model, mut receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);
    let unit_path = "ch/main".to_string();
    let adapter = test_runtime.spawn(fake_adapter_fails_action(
        directory.path().join("runtime.json"),
    ));

    test_runtime.block_on(async {
        wait_for_model_connection(&mut model, &mut receiver, true).await;
        model
            .start_learning(LearningMode::ByStep, &unit_path)
            .unwrap();
        model
            .session()
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_model(&mut model, &mut receiver, |model| {
            model.state().learning().phase == cynapse_client_core::LearningPhase::Failed
        })
        .await;
        assert_eq!(model.state().learning().mode, Some(LearningMode::ByStep));
        assert!(model.is_learning());
        assert!(model.can_retry());

        model.session().retry_playback().unwrap();
        wait_for_model(&mut model, &mut receiver, |model| {
            model.state().learning().phase == cynapse_client_core::LearningPhase::Failed
        })
        .await;
        assert_eq!(model.state().learning().mode, Some(LearningMode::ByStep));
        let _ = model.session().stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn an_error_without_a_position_hides_retry() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);
    model.apply(ClientEvent::Connection(ConnectionState::Connected {
        adapter: "vscode".to_string(),
    }));
    model
        .start_learning(LearningMode::ByStep, "ch_units/first_unit")
        .unwrap();

    model.apply(ClientEvent::Playback(Box::new(PlaybackState::Error {
        code: "persistence_failed".to_string(),
        message: "database is locked".to_string(),
        position: None,
        interruption: None,
    })));

    assert_eq!(
        model.state().learning().phase,
        cynapse_client_core::LearningPhase::Failed
    );
    assert_eq!(model.state().learning().mode, Some(LearningMode::ByStep));
    assert!(!model.can_retry());
    assert!(model.session().retry_playback().is_err());
}

#[test]
fn advances_to_the_next_unit_after_a_unit_completes() {
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
        let skipped = service.skip_challenge(&unit_path, "check").await.unwrap();
        assert_eq!(skipped.outcome, ChallengeOutcome::Skipped);
        service.advance_step().unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        service.advance_unit(LearningMode::ByStep).await.unwrap();
        let playback = wait_for_playback(&mut receiver, |playback| {
            matches!(
                playback,
                PlaybackState::UnitComplete { position }
                    if position.unit_path == "ch/next"
            )
        })
        .await;
        assert_eq!(
            playback_position(&playback).map(|position| position.unit_path.as_str()),
            Some("ch/next")
        );
        let _ = service.stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn start_preflight_failure_happens_before_seed_copy() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_start_environment_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package.clone())).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let adapter = test_runtime.spawn(fake_adapter_fails_environment(
        directory.path().join("runtime.json"),
    ));

    test_runtime.block_on(async {
        wait_for_connection(&mut receiver, true).await;
        let error = service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .expect_err("a failed preflight must not start the Unit");
        assert!(error.contains("boom"), "{error}");
        assert!(
            !package.join("workspace/ch/main/seed.txt").exists(),
            "preflight failure must happen before Seed copy"
        );
        let _ = service.stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn a_failed_next_unit_plan_keeps_the_committed_unit_and_idles_playback() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_advance_environment_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package.clone())).unwrap();
    let first = state.selected_unit.expect("startup unit").path;
    let adapter = test_runtime.spawn(fake_adapter_closes_after_environment(
        directory.path().join("runtime.json"),
        "ch/second",
    ));

    test_runtime.block_on(async {
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &first)
            .await
            .unwrap();
        let playback = wait_for_playback(&mut receiver, |playback| {
            matches!(
                playback,
                PlaybackState::StepComplete { .. } | PlaybackState::UnitComplete { .. }
            )
        })
        .await;
        if matches!(playback, PlaybackState::StepComplete { .. }) {
            service.advance_step().unwrap();
            wait_for_playback(&mut receiver, |playback| {
                matches!(playback, PlaybackState::UnitComplete { .. })
            })
            .await;
        }
        let error = service
            .advance_unit(LearningMode::ByStep)
            .await
            .expect_err("a failed plan replacement must be reported");
        assert_eq!(error.text(), t("error.next_unit_plan_unavailable"));
        let state = service.course_state().unwrap();
        assert_eq!(
            state.selected_unit.expect("committed next unit").path,
            "ch/second"
        );
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::Idle)
        })
        .await;
        let _ = service.stop_learning().await;
    });

    adapter.abort();
}

#[test]
fn advance_preflight_failure_happens_before_seed_copy() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_advance_environment_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package.clone())).unwrap();
    let first = state.selected_unit.expect("startup unit").path;
    let adapter = test_runtime.spawn(fake_adapter_fails_environment(
        directory.path().join("runtime.json"),
    ));

    test_runtime.block_on(async {
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &first)
            .await
            .unwrap();
        let playback = wait_for_playback(&mut receiver, |playback| {
            matches!(
                playback,
                PlaybackState::StepComplete { .. } | PlaybackState::UnitComplete { .. }
            )
        })
        .await;
        if matches!(playback, PlaybackState::StepComplete { .. }) {
            service.advance_step().unwrap();
            wait_for_playback(&mut receiver, |playback| {
                matches!(playback, PlaybackState::UnitComplete { .. })
            })
            .await;
        }
        let error = service
            .advance_unit(LearningMode::ByStep)
            .await
            .expect_err("a failed preflight must not enter the next Unit");
        assert!(error.text().contains("boom"), "{error}");
        assert!(
            !package.join("workspace/ch/second/seed.txt").exists(),
            "advance preflight failure must happen before Seed copy"
        );
        let _ = service.stop_learning().await;
    });

    adapter.abort();
}
