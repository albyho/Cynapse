//! Course lifecycle, transition, reset, and cleanup tests.

use super::support::*;
use super::*;
use cynapse_client_i18n::t;
use std::fs;
use std::time::Duration;
use tokio::sync::mpsc::{self};
use tokio::time::timeout;

#[test]
fn opening_a_course_publishes_and_clears_a_transition() {
    let directory = tempfile::tempdir().unwrap();
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");

    futures::executor::block_on(service.open_course(package)).unwrap();

    let mut started = false;
    let mut finished = false;
    while let Ok(event) = receiver.try_recv() {
        if let ClientEvent::Transition(transition) = event {
            match transition {
                Some(transition) => {
                    assert_eq!(transition.kind, ClientTransitionKind::Opening);
                    started = true;
                }
                None => finished = true,
            }
        }
    }
    assert!(started, "opening must publish a running transition");
    assert!(finished, "opening must clear the transition when done");
}

/// The service stores the resolved package path, so an expected path has to be
/// resolved the same way. On Windows `fs::canonicalize` also expands 8.3 short
/// names (`RUNNER~1`), which temp directories on CI runners carry, and it adds a
/// `\\?\` prefix that the service strips.
fn resolved(path: &std::path::Path) -> String {
    let text = fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .to_string();
    text.strip_prefix("\\\\?\\")
        .map(str::to_string)
        .unwrap_or(text)
}

#[test]
fn reopening_a_course_reports_a_restart_failure() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let first = write_adapterless_course(&directory.path().join("first"));
    let second = write_adapterless_course(&directory.path().join("second"));
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();

    runtime.block_on(async {
        service.open_course(first).await.unwrap();
        let runtime_path = directory.path().join("runtime.json");
        assert!(runtime_path.is_file());
        // A directory at runtime.json makes the next discovery write fail.
        fs::remove_file(&runtime_path).unwrap();
        fs::create_dir(&runtime_path).unwrap();

        let error = service
            .open_course(second.clone())
            .await
            .expect_err("a failed restart must be reported");
        assert!(!error.is_empty());
        assert_eq!(
            service.course_state().unwrap().active_course_path,
            Some(resolved(&second))
        );
    });
}

#[test]
fn starting_learning_restarts_a_missing_local_server() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_adapterless_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();

    runtime.block_on(async {
        let state = service.open_course(package).await.unwrap();
        let unit_path = state.selected_unit.expect("startup unit").path;
        service.stop_server().await;
        assert!(service.server_commands().is_err());

        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        let _ = service.stop_learning().await;
    });
}

#[test]
fn queues_a_course_operation_until_the_active_transition_finishes() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let ticket = service
        .inner
        .transitions
        .try_transition(ClientTransitionRequirement::AdapterIndependent)
        .unwrap();

    runtime.block_on(async {
        let select = service.select_unit(&unit_path);
        let finish = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            service.finish_transition(ticket);
        };
        let (state, ()) = tokio::join!(select, finish);
        let state = state.unwrap();
        assert_eq!(
            state.selected_unit.map(|unit| unit.path).as_deref(),
            Some(unit_path.as_str())
        );
    });
}

#[test]
fn a_stop_barrier_rejects_new_course_operations() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let StopTransitionRequest::Started(stop) = service.inner.transitions.request_stop() else {
        panic!("the first stop must start a barrier");
    };

    assert_eq!(
        futures::executor::block_on(service.select_unit(&unit_path)).unwrap_err(),
        t("error.stop_in_progress")
    );

    service.inner.transitions.finish_stop(&stop);
}

#[test]
fn closes_a_course_and_keeps_recent_history() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");

    futures::executor::block_on(service.open_course(package)).unwrap();
    let state = futures::executor::block_on(service.close_course()).unwrap();
    assert!(state.active_course.is_none());
    assert!(state.active_course_path.is_none());
    assert!(state.selected_unit.is_none());
    assert!(
        state
            .recent_courses
            .iter()
            .any(|course| course.path.ends_with("multi-unit-demo"))
    );
}

#[test]
fn clearing_recent_courses_keeps_the_active_course() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");

    futures::executor::block_on(service.open_course(package)).unwrap();
    let state = service.clear_recent_courses().unwrap();

    assert!(state.recent_courses.is_empty());
    assert_eq!(
        state
            .active_course
            .as_ref()
            .map(|course| course.id.as_str()),
        Some("multi_unit_demo")
    );
}

#[test]
fn closing_a_course_persists_course_switch_recovery_without_waiting_for_cancel() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package.clone())).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let adapter = test_runtime.spawn(fake_adapter_holds_terminal_command(
        directory.path().join("runtime.json"),
    ));

    test_runtime.block_on(async {
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(
                playback,
                PlaybackState::Playing { position, .. } if position.action_index == 2
            )
        })
        .await;

        timeout(Duration::from_secs(2), service.close_course())
            .await
            .expect("close must not wait for an Adapter cancel ACK")
            .unwrap();

        let state = service.open_course(package).await.unwrap();
        let recovery = state
            .active_course
            .expect("active course")
            .recovery
            .expect("course-switch recovery");
        assert_eq!(
            recovery.reason,
            cynapse_client_core::PlaybackInterruptionReason::CourseSwitch
        );
        assert_eq!(recovery.command, "node --test");
        assert!(recovery.output.is_none());
        assert!(recovery.exit_code.is_none());
    });

    adapter.abort();
}

#[test]
fn reopening_the_course_while_playing_persists_course_switch_recovery() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package.clone())).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let adapter = test_runtime.spawn(fake_adapter_holds_terminal_command(
        directory.path().join("runtime.json"),
    ));

    test_runtime.block_on(async {
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(
                playback,
                PlaybackState::Playing { position, .. } if position.action_index == 2
            )
        })
        .await;

        let state = timeout(Duration::from_secs(2), service.open_course(package))
            .await
            .expect("open must not wait for an Adapter cancel ACK")
            .unwrap();
        let recovery = state
            .active_course
            .expect("active course")
            .recovery
            .expect("course-switch recovery");
        assert_eq!(
            recovery.reason,
            cynapse_client_core::PlaybackInterruptionReason::CourseSwitch
        );
    });

    adapter.abort();
}

#[test]
fn starts_and_stops_a_unit_without_an_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/manual-boundary-demo");

    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit = state.selected_unit.expect("startup unit");
    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, &unit.path)).unwrap();
    assert!(directory.path().join("runtime.json").exists());
    futures::executor::block_on(service.stop_learning()).unwrap();
}

#[test]
fn start_without_a_required_adapter_reports_the_connection_problem_every_time() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;

    futures::executor::block_on(async {
        let first = service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .expect_err("a required Adapter is missing");
        let second = service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .expect_err("a second Start must report the same problem");
        assert_eq!(first, cynapse_client_core::adapter_disconnected_message());
        assert_eq!(second, first);
        assert_ne!(second, "a learning mode is already active");
    });

    let state = service.course_state().unwrap();
    assert_eq!(
        state.selected_unit.as_ref().map(|unit| unit.path.as_str()),
        Some(unit_path.as_str())
    );
    let unit = state.selected_unit.unwrap();
    assert!(unit.steps.iter().all(|step| !step.started));
}

#[test]
fn resets_a_unit_and_clears_playback() {
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
        let state = service.reset_unit(&unit_path).await.unwrap();
        assert!(state.active_course.is_some());
    });
    test_runtime.block_on(wait_for_playback(&mut receiver, |playback| {
        matches!(playback, PlaybackState::Ready { position: None })
    }));
    adapter.abort();
}

#[test]
fn resetting_a_unit_while_disconnected_keeps_the_server_reachable() {
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
    let adapter = test_runtime.spawn(fake_adapter(runtime_path.clone()));

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

        service.reset_unit(&unit_path).await.unwrap();
        assert!(
            runtime_path.exists(),
            "a disconnected Unit reset must keep the discovery file"
        );

        let reconnected = test_runtime.spawn(fake_adapter(runtime_path.clone()));
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::StepComplete { .. })
        })
        .await;
        reconnected.abort();
    });
}

#[test]
fn a_unit_reset_deletes_workspace_files_only_after_the_adapter_acks() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package.clone())).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let workspace_file = package.join("workspace/ch/main/notes.txt");

    let (seen, mut seen_rx) = mpsc::unbounded_channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let adapter = test_runtime.spawn(fake_adapter_holds_workspace_reset(
        directory.path().join("runtime.json"),
        seen,
        release_rx,
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
        fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
        fs::write(&workspace_file, "learner work\n").unwrap();

        let reset = service.reset_unit(&unit_path);
        tokio::pin!(reset);
        let release_when_seen = async {
            seen_rx
                .recv()
                .await
                .expect("workspace_reset was never sent");
            assert!(
                workspace_file.exists(),
                "workspace files were deleted before the Adapter ACK"
            );
            release.send(()).expect("release the held ACK");
        };
        let (result, ()) = tokio::join!(reset, release_when_seen);
        result.unwrap();
        assert!(!workspace_file.exists());
    });
    adapter.abort();
}

#[test]
fn a_failed_workspace_reset_keeps_unit_workspace_files() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_challenge_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package.clone())).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    let workspace_file = package.join("workspace/ch/main/notes.txt");
    let adapter = test_runtime.spawn(fake_adapter_rejects_workspace_reset(
        directory.path().join("runtime.json"),
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
        fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
        fs::write(&workspace_file, "learner work\n").unwrap();

        let error = service.reset_unit(&unit_path).await.unwrap_err();
        assert!(error.contains("Notebook still open"), "{error}");
        assert!(workspace_file.exists());
    });
    adapter.abort();
}

#[test]
fn resets_a_unit_from_a_non_tokio_driver() {
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
    });

    // The reset future is driven by a non-Tokio executor; its runtime-created
    // timeouts and workspace cleanup must not require an ambient reactor.
    let state = drive_without_runtime(service.reset_unit(&unit_path)).unwrap();
    assert!(state.active_course.is_some());
    test_runtime.block_on(wait_for_playback(&mut receiver, |playback| {
        matches!(playback, PlaybackState::Ready { position: None })
    }));
    adapter.abort();
}

#[test]
fn resets_the_course_and_selects_its_first_unit_again() {
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
        let state = service.reset_course().await.unwrap();
        assert_eq!(
            state.selected_unit.as_ref().map(|unit| unit.path.as_str()),
            Some("ch/main")
        );
    });
    test_runtime.block_on(wait_for_playback(&mut receiver, |playback| {
        matches!(playback, PlaybackState::Ready { position: None })
    }));
    adapter.abort();
}
