use super::*;

/// Position serialization, adapterless playback, run ids, and play guards.

#[tokio::test]
async fn wait_action_delays_locally_without_being_sent_to_the_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: Action::Wait(crate::course::WaitAction { duration_ms: 75 }),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
            PlaybackAction {
                action: Action::CreateFile(CreateFileAction {
                    path: "after-wait.txt".to_string(),
                    create_parents: true,
                }),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
        ],
    )
    .await;
    let mut playback = handle.playback.clone();
    let started = tokio::time::Instant::now();

    handle.commands.play().unwrap();
    let next = receive_action(&mut socket).await;

    assert_eq!(next.payload["type"], "CreateFile");
    assert!(started.elapsed() >= Duration::from_millis(50));
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Playing {
                position: PlaybackPosition {
                    action_index: 1,
                    ..
                },
                ..
            }
        )
    })
    .await;

    send_ack(&mut socket, &next.id, "done").await;
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::UnitComplete { .. })
    })
    .await;
    handle.shutdown().await;
}

#[tokio::test]
async fn stopping_during_a_final_wait_pauses() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, _socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: Action::Wait(crate::course::WaitAction {
                duration_ms: 10_000,
            }),
            step_id: "step".to_string(),
            narration: ActionNarration::default(),
        }],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::Playing { .. })
    })
    .await;
    handle.commands.cancel().unwrap();

    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(state, PlaybackState::Paused { .. })
        }),
    )
    .await
    .unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn by_step_learning_skips_a_final_wait() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, _socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: Action::Wait(crate::course::WaitAction {
                duration_ms: 10_000,
            }),
            step_id: "step".to_string(),
            narration: ActionNarration::default(),
        }],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.set_skip_unit_final_wait(true).unwrap();
    handle.commands.play().unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(state, PlaybackState::UnitComplete { .. })
        }),
    )
    .await
    .expect("a final Wait must be skipped in by-step learning");
    handle.shutdown().await;
}

#[tokio::test]
async fn continuous_learning_still_honors_a_final_wait() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, _socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: Action::Wait(crate::course::WaitAction { duration_ms: 75 }),
            step_id: "step".to_string(),
            narration: ActionNarration::default(),
        }],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::Playing { .. })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(matches!(*playback.borrow(), PlaybackState::Playing { .. }));
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(state, PlaybackState::UnitComplete { .. })
        }),
    )
    .await
    .expect("continuous learning must honor the final Wait");
    handle.shutdown().await;
}

#[tokio::test]
async fn forcing_skip_waits_skips_a_non_final_wait() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, _socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: Action::Wait(crate::course::WaitAction {
                    duration_ms: 10_000,
                }),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
            PlaybackAction {
                action: Action::Wait(crate::course::WaitAction {
                    duration_ms: 10_000,
                }),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
        ],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.set_skip_waits(true).unwrap();
    handle.commands.play().unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(state, PlaybackState::UnitComplete { .. })
        }),
    )
    .await
    .expect("forced skip_waits must skip every Wait delay");
    handle.shutdown().await;
}

#[tokio::test]
async fn stopping_during_a_non_final_wait_pauses_without_dispatching_the_next_action() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: Action::Wait(crate::course::WaitAction {
                    duration_ms: 10_000,
                }),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
            PlaybackAction {
                action: Action::CreateFile(CreateFileAction {
                    path: "after-wait.txt".to_string(),
                    create_parents: true,
                }),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
        ],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::Playing { .. })
    })
    .await;
    handle.commands.cancel().unwrap();

    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::Paused { .. })
    })
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), receive_action(&mut socket))
            .await
            .is_err()
    );
    handle.shutdown().await;
}

#[test]
fn playback_boundary_states_serialize_position_for_the_client() {
    let position = PlaybackPosition {
        parent_id: "ch_01".to_string(),
        parent_title: "Getting Started".to_string(),
        unit_path: "unit_01".to_string(),
        unit_title: "First Unit".to_string(),
        step_id: "write_hello".to_string(),
        step_title: "Hello World".to_string(),
        step_started: false,
        step_index: 0,
        step_total: 2,
        unit_step_index: 0,
        unit_step_total: 1,
        action_index: 1,
        action_total: 1,
        overall_action_index: 1,
        overall_action_total: 2,
    };
    let state = PlaybackState::StepComplete {
        position: position.clone(),
        continuation: None,
        requires_human: false,
    };
    let value = serde_json::to_value(state).unwrap();

    assert_eq!(value["state"], "step_complete");
    assert_eq!(value["position"]["parent_title"], "Getting Started");
    assert_eq!(value["position"]["unit_title"], "First Unit");
    assert_eq!(value["position"]["step_title"], "Hello World");
    assert_eq!(value["position"]["step_index"], 0);
    assert_eq!(value["position"]["step_total"], 2);
    assert_eq!(value["position"]["overall_action_total"], 2);

    let unit_complete = serde_json::to_value(PlaybackState::UnitComplete { position }).unwrap();
    assert_eq!(unit_complete["state"], "unit_complete");
}

#[tokio::test]
async fn adapterless_course_runs_present_steps_without_an_external_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let mut step = playback_step("only", "Only", vec![Action::Present]);
    step.actions[0].narration = ActionNarration {
        during: Some(ActionNarrationPhaseClips {
            clips: vec![action_narration_clip_for_phase(
                "This explanation needs no external adapter.",
            )],
        }),
        ..ActionNarration::default()
    };
    let handle = start_server_with_path_and_resume_for_adapter(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![step],
        None,
        AdapterTarget::Local,
    )
    .await
    .unwrap();
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle.commands.narration_started(clip.clone()).unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                continuation: Some(continuation),
                ..
            } if continuation.clip == clip
        )
    })
    .await;
    handle.commands.narration_ended(clip).unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::UnitComplete { .. })
    })
    .await;

    handle.shutdown().await;
}

#[tokio::test]
async fn actions_in_one_step_share_the_same_step_run_id() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: insert_text("first"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    before: Some(ActionNarrationPhaseClips {
                        clips: vec![action_narration_clip_for_phase("before-first")],
                    }),
                    ..ActionNarration::default()
                },
            },
            PlaybackAction {
                action: insert_text("second"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    during: Some(ActionNarrationPhaseClips {
                        clips: vec![action_narration_clip_for_phase("during-second")],
                    }),
                    ..ActionNarration::default()
                },
            },
        ],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let first_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::Before, 0, 0).await;
    handle
        .commands
        .narration_started(first_clip.clone())
        .unwrap();
    handle.commands.narration_ended(first_clip.clone()).unwrap();
    let first_action = receive_action(&mut socket).await;
    assert_eq!(first_action.unit_path.as_deref(), Some("unit"));
    send_ack(&mut socket, &first_action.id, "done").await;

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Narrating { clip, .. }
                if clip.phase == ActionNarrationPhase::During && clip.action_index == 1
        )
    })
    .await;
    let PlaybackState::Narrating {
        clip: second_clip, ..
    } = playback.borrow().clone()
    else {
        unreachable!();
    };

    assert_eq!(second_clip.step_run_id, first_clip.step_run_id);
    assert_ne!(second_clip.action_index, first_clip.action_index);

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn ignores_a_second_play_while_an_action_is_in_flight() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![insert_text("only")]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;

    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );

    send_ack(&mut socket, &action.id, "done").await;
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
