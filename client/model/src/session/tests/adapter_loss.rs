//! Adapter loss semantics and reconnect tests.

use super::support::*;
use super::*;
use std::time::Duration;

#[test]
fn adapter_loss_blocks_check_and_advance_at_a_step_checkpoint() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course_with_timeout(directory.path(), 100);
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

        adapter.abort();
        wait_for_connection(&mut receiver, false).await;

        let state_before = service.course_state().unwrap();
        let playback_before = service.playback_state().unwrap();
        assert!(matches!(
            playback_before,
            PlaybackState::StepComplete { .. }
        ));

        let check_error = service
            .check_challenge(&unit_path, "check", true)
            .await
            .expect_err("Check must be blocked while the Adapter is gone");
        assert_eq!(
            check_error,
            cynapse_client_core::adapter_disconnected_message()
        );
        let advance_error = service
            .advance_step()
            .expect_err("Next must be blocked while the Adapter is gone");
        assert_eq!(
            advance_error,
            cynapse_client_core::adapter_disconnected_message()
        );

        assert_eq!(service.course_state().unwrap(), state_before);
        assert_eq!(service.playback_state().unwrap(), playback_before);
        let _ = service.stop_learning().await;
    });
}

#[test]
fn adapter_loss_blocks_advance_to_the_next_unit() {
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
        service.skip_challenge(&unit_path, "check").await.unwrap();
        service.advance_step().unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        adapter.abort();
        wait_for_connection(&mut receiver, false).await;

        let state_before = service.course_state().unwrap();
        let playback_before = service.playback_state().unwrap();
        assert!(matches!(
            playback_before,
            PlaybackState::UnitComplete { .. }
        ));

        let error = service
            .advance_unit(LearningMode::ByStep)
            .await
            .expect_err("next Unit must be blocked while the Adapter is gone");
        assert!(error.is_adapter_disconnected(), "{error}");
        assert_eq!(service.course_state().unwrap(), state_before);
        assert_eq!(service.playback_state().unwrap(), playback_before);
        let _ = service.stop_learning().await;
    });
}

#[test]
fn a_failure_after_an_adapter_stop_cannot_revive_failed_learning() {
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
    model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));

    model.fail_learning(cynapse_client_core::adapter_disconnected_message());

    assert_eq!(
        model.state().learning().phase,
        cynapse_client_core::LearningPhase::Idle
    );
    assert_eq!(model.state().learning().mode, None);
    assert!(!model.is_learning());
}

#[test]
fn adapter_disconnect_requires_an_explicit_restart_after_reconnect() {
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
    let runtime_path = directory.path().join("runtime.json");
    let disconnecting =
        test_runtime.spawn(fake_adapter_disconnect_after_action(runtime_path.clone()));

    test_runtime.block_on(async {
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(
                playback,
                PlaybackState::Paused {
                    interruption,
                    ..
                } if interruption.reason
                    == cynapse_client_core::PlaybackInterruptionReason::AdapterDisconnected
            )
        })
        .await;

        let error = service
            .retry_playback()
            .expect_err("Retry must be blocked while the Adapter is gone");
        assert_eq!(error, cynapse_client_core::adapter_disconnected_message());

        let reconnect = test_runtime.spawn(fake_adapter(runtime_path.clone()));
        wait_for_connection(&mut receiver, true).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            matches!(
                service.playback_state().unwrap(),
                PlaybackState::Paused { .. }
            ),
            "reconnect must not resume playback automatically"
        );

        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        let _ = service.stop_learning().await;
        reconnect.abort();
    });

    disconnecting.abort();
}

#[test]
fn adapter_loss_does_not_stop_a_unit_without_required_adapters() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let package =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/manual-boundary-demo");
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);
    model
        .start_learning(LearningMode::ByStep, "ch/unit")
        .expect("a Unit without required Adapters can start");
    assert!(!model.adapter_loss_ends_learning(&ConnectionState::WaitingForAdapter));
    model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));

    assert!(model.is_learning());
    assert_eq!(model.state().learning().mode, Some(LearningMode::ByStep));

    let paused = PlaybackState::Paused {
        position: PlaybackPosition {
            unit_path: "ch/unit".to_string(),
            ..PlaybackPosition::default()
        },
        interruption: cynapse_client_core::PlaybackInterruption {
            reason: cynapse_client_core::PlaybackInterruptionReason::AdapterDisconnected,
            exit_code: None,
            output: None,
            output_truncated: None,
        },
    };
    assert!(!model.playback_marks_adapter_loss(&paused));
    model.apply(ClientEvent::Playback(Box::new(paused)));
    assert!(model.is_learning());
    assert_eq!(model.state().learning().mode, Some(LearningMode::ByStep));
}

#[test]
fn adapter_loss_during_starting_transition_does_not_stop_learning() {
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
    model.apply(ClientEvent::Transition(Some(ClientTransitionView {
        id: "start".to_string(),
        kind: ClientTransitionKind::Starting,
        phase: ClientTransitionPhase::Running,
        cancellable: true,
        stage: None,
        message: None,
    })));

    assert!(!model.adapter_loss_ends_learning(&ConnectionState::WaitingForAdapter));
    model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));

    assert!(model.is_learning());
    assert_eq!(model.state().learning().mode, Some(LearningMode::ByStep));

    // Once Start ends, a still-disconnected Adapter is a real loss.
    model.apply(ClientEvent::Transition(None));
    model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));

    assert!(!model.is_learning());
    assert!(
        model
            .state()
            .warnings()
            .iter()
            .any(|warning| warning.is_adapter_disconnected())
    );
}

#[test]
fn playback_adapter_loss_warns_before_the_connection_waiting_event() {
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
    let generation = model.learning_generation();
    let paused = PlaybackState::Paused {
        position: PlaybackPosition::default(),
        interruption: cynapse_client_core::PlaybackInterruption {
            reason: cynapse_client_core::PlaybackInterruptionReason::AdapterDisconnected,
            exit_code: None,
            output: None,
            output_truncated: None,
        },
    };

    assert!(model.playback_marks_adapter_loss(&paused));
    model.apply(ClientEvent::Playback(Box::new(paused)));

    assert!(!model.is_learning());
    assert_eq!(model.learning_generation(), generation + 1);
    assert!(
        model
            .state()
            .warnings()
            .iter()
            .any(|warning| warning.is_adapter_disconnected())
    );

    model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));

    assert_eq!(model.learning_generation(), generation + 1);
    assert_eq!(
        model
            .state()
            .warnings()
            .iter()
            .filter(|warning| warning.is_adapter_disconnected())
            .count(),
        1,
        "the later Connection event must not duplicate the warning"
    );
}

#[test]
fn adapter_loss_ends_learning_only_for_units_with_required_adapters() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);
    let connected = ConnectionState::Connected {
        adapter: "vscode".to_string(),
    };
    model.apply(ClientEvent::Connection(connected.clone()));
    model
        .start_learning(LearningMode::ByStep, "ch_units/first_unit")
        .unwrap();

    assert!(!model.adapter_loss_ends_learning(&connected));
    assert!(model.adapter_loss_ends_learning(&ConnectionState::WaitingForAdapter));
}

#[test]
fn a_stale_start_failure_does_not_stop_a_newer_start() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();
    let package =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/manual-boundary-demo");
    let state = futures::executor::block_on(model.session().open_course(package)).unwrap();
    model.apply_course_state(state);

    model
        .start_learning(LearningMode::ByStep, "ch/unit")
        .unwrap();
    let stale = model.learning_generation();
    model.stop_learning_local();
    model
        .start_learning(LearningMode::Continuous, "ch/unit")
        .unwrap();

    assert!(!model.stop_learning_after_start_failure(stale));
    assert_eq!(
        model.state().learning().mode,
        Some(LearningMode::Continuous)
    );

    let current = model.learning_generation();
    assert!(model.stop_learning_after_start_failure(current));
    assert_eq!(model.state().learning().mode, None);
}

#[test]
fn adapter_disconnect_ends_learning_with_stop_semantics() {
    for mode in [LearningMode::ByStep, LearningMode::Continuous] {
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
        let runtime_path = directory.path().join("runtime.json");
        let disconnecting =
            test_runtime.spawn(fake_adapter_disconnect_after_action(runtime_path.clone()));

        test_runtime.block_on(async {
            wait_for_model_connection(&mut model, &mut receiver, true).await;
            model.start_learning(mode, &unit_path).unwrap();
            model
                .session()
                .begin_learning(mode, &unit_path)
                .await
                .unwrap();

            wait_for_model(&mut model, &mut receiver, |model| {
                !model.is_learning()
                    && matches!(model.connection(), ConnectionState::WaitingForAdapter)
            })
            .await;
            assert_eq!(
                model.state().learning().phase,
                cynapse_client_core::LearningPhase::Idle
            );
            assert_eq!(model.state().learning().mode, None);
            assert_eq!(model.selected_unit_path(), Some(unit_path.as_str()));
            let course = model
                .session()
                .course_state()
                .unwrap()
                .active_course
                .expect("active course");
            assert_eq!(
                course.current_unit_path.as_deref(),
                Some(unit_path.as_str())
            );
            assert_eq!(course.current_step_id.as_deref(), Some("check"));

            let reconnect = test_runtime.spawn(fake_adapter(runtime_path.clone()));
            wait_for_model_connection(&mut model, &mut receiver, true).await;
            model.start_learning(mode, &unit_path).unwrap();
            model
                .session()
                .begin_learning(mode, &unit_path)
                .await
                .unwrap();
            let _ = model.session().stop_learning().await;
            reconnect.abort();
        });

        disconnecting.abort();
    }
}
