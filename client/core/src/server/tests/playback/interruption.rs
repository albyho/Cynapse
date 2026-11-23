use super::*;

/// Manual progression, stop, and adapter disconnect cutting tails.

#[tokio::test]
async fn manual_progression_interrupts_the_action_narration_tail() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = playback_step("first", "First", vec![insert_text("first")]);
    first.actions[0].narration = ActionNarration {
        during: Some(ActionNarrationPhaseClips {
            clips: vec![action_narration_clip_for_phase("during")],
        }),
        ..ActionNarration::default()
    };
    let handle = start_server_with_path(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![
            first,
            playback_step("second", "Second", vec![insert_text("second")]),
        ],
    )
    .await
    .unwrap();
    let mut playback = handle.playback.clone();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let narration_clip =
        wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();
    let first_action = receive_action(&mut socket).await;
    send_ack(&mut socket, &first_action.id, "done").await;
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                continuation: Some(_),
                ..
            }
        )
    })
    .await;

    handle.commands.play().unwrap();
    let next = receive_action(&mut socket).await;
    assert_eq!(next.payload["text"], "second");
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Playing { position, .. } if position.step_id == "second"
        )
    })
    .await;

    handle
        .commands
        .narration_ended(narration_clip.clone())
        .unwrap();
    assert!(
        matches!(
            &*playback.borrow(),
            PlaybackState::Playing { position, .. } if position.step_id == "second"
        ),
        "a stale tail event must not overwrite the next Step"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "a stale tail event must not dispatch another Action"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn jupyterlab_kernel_restart_fallback_marks_the_pause_reason() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: insert_text("first"),
            step_id: "step".to_string(),
            narration: ActionNarration::default(),
        }],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    handle
        .commands
        .cancel_with_reason(CancelReason::UserStop)
        .unwrap();
    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.kind, MessageType::Cancel);

    send_ack_payload(
        &mut socket,
        &action.id,
        serde_json::json!({
            "status": "cancelled",
            "code": "kernel_restarted",
            "message": "Kernel restarted; variables cleared",
        }),
    )
    .await;

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Paused {
                position,
                interruption: PlaybackInterruption {
                    reason: PlaybackInterruptionReason::JupyterLabKernelRestarted,
                    ..
                },
            } if position.step_id == "step"
        )
    })
    .await;

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn stop_interrupts_a_step_complete_action_narration_tail() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = playback_step("first", "First", vec![insert_text("first")]);
    first.actions[0].narration = ActionNarration {
        during: Some(ActionNarrationPhaseClips {
            clips: vec![action_narration_clip_for_phase("during")],
        }),
        ..ActionNarration::default()
    };
    let handle = start_server_with_path(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![
            first,
            playback_step("second", "Second", vec![insert_text("second")]),
        ],
    )
    .await
    .unwrap();
    let mut playback = handle.playback.clone();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let narration_clip =
        wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                continuation: Some(_),
                ..
            }
        )
    })
    .await;

    handle
        .commands
        .cancel_with_reason(CancelReason::UserStop)
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Paused {
                    position,
                    interruption: PlaybackInterruption {
                        reason: PlaybackInterruptionReason::UserStop,
                        ..
                    },
                } if position.step_id == "first"
            )
        }),
    )
    .await
    .expect("Stop should interrupt the active narration tail");

    handle
        .commands
        .narration_ended(narration_clip.clone())
        .unwrap();
    assert!(matches!(
        &*playback.borrow(),
        PlaybackState::Paused { position, .. } if position.step_id == "first"
    ));

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn adapter_disconnect_interrupts_a_step_complete_narration_tail() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = playback_step("first", "First", vec![insert_text("first")]);
    first.actions[0].narration = ActionNarration {
        during: Some(ActionNarrationPhaseClips {
            clips: vec![action_narration_clip_for_phase("during")],
        }),
        ..ActionNarration::default()
    };
    let handle = start_server_with_path(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![
            first,
            playback_step("second", "Second", vec![insert_text("second")]),
        ],
    )
    .await
    .unwrap();
    let mut playback = handle.playback.clone();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let narration_clip =
        wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                continuation: Some(_),
                ..
            }
        )
    })
    .await;

    socket.close(None).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Paused {
                    position,
                    interruption: PlaybackInterruption {
                        reason: PlaybackInterruptionReason::AdapterDisconnected,
                        ..
                    },
                } if position.step_id == "first"
            )
        }),
    )
    .await
    .expect("disconnect should retain the Step as an interrupted pause");

    handle.shutdown().await;
}
