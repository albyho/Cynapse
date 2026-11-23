use super::*;

/// Narration and persistence failures preserving the step for retry.

#[tokio::test]
async fn action_before_narration_failure_preserves_the_step_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let handle = start_server_with_path(
        workspace,
        directory.path().join("runtime.json"),
        vec![PlaybackStep {
            parent_id: "ch".to_string(),
            parent_title: "Grouping".to_string(),
            unit_path: "unit".to_string(),
            unit_title: "Unit".to_string(),
            step_id: "step".to_string(),
            step_title: "Step".to_string(),
            requires_human: false,
            challenge: false,
            actions: vec![PlaybackAction {
                action: insert_text("after retry"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    before: Some(ActionNarrationPhaseClips {
                        clips: vec![action_narration_clip()],
                    }),
                    ..ActionNarration::default()
                },
            }],
        }],
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let mut playback = handle.playback.clone();
    let first_instance =
        wait_for_narrating(&mut playback, ActionNarrationPhase::Before, 0, 0).await;
    handle
        .commands
        .narration_failed(first_instance.clone(), "audio decode failed")
        .unwrap();
    let failed = wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Error {
                code,
                position: Some(PlaybackPosition {
                    unit_path,
                    step_id,
                    ..
                }),
                ..
            } if code == "narration_failed"
                && unit_path == "unit"
                && step_id == "step"
        )
    });
    tokio::time::timeout(Duration::from_secs(2), failed)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "media failure must not dispatch the Action"
    );

    handle.commands.play().unwrap();
    let retry_instance =
        wait_for_narrating(&mut playback, ActionNarrationPhase::Before, 0, 0).await;
    assert_ne!(retry_instance, first_instance);
    handle
        .commands
        .narration_started(retry_instance.clone())
        .unwrap();
    handle
        .commands
        .narration_ended(retry_instance.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    assert_eq!(action.payload["text"], "after retry");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn during_narration_failure_cancels_the_active_action_and_preserves_the_step() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: insert_text("first"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    during: Some(ActionNarrationPhaseClips {
                        clips: vec![action_narration_clip_for_phase("during")],
                    }),
                    ..ActionNarration::default()
                },
            },
            PlaybackAction {
                action: insert_text("second"),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
        ],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let narration_clip =
        wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    handle
        .commands
        .narration_failed(narration_clip.clone(), "during media failed")
        .unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Error {
                position: Some(PlaybackPosition { step_id, .. }),
                ..
            } if step_id == "step"
        )
    })
    .await;
    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.kind, MessageType::Cancel);
    assert_eq!(cancel.payload["target_id"], action.id);

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn persistence_failure_cancels_the_active_action_and_reports_an_error() {
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
    handle.commands.persistence_failed("disk is full").unwrap();

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Error {
                code,
                message,
                position: Some(PlaybackPosition { step_id, .. }),
                ..
            } if code == "persistence_failed"
                && message == "disk is full"
                && step_id == "step"
        )
    })
    .await;

    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.kind, MessageType::Cancel);
    assert_eq!(cancel.payload["target_id"], action.id);
    send_ack(&mut socket, &action.id, "cancelled").await;
    assert!(matches!(
        playback.borrow().clone(),
        PlaybackState::Error { code, .. } if code == "persistence_failed"
    ));

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn after_narration_does_not_start_after_action_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: insert_text("first"),
            step_id: "step".to_string(),
            narration: ActionNarration {
                after: Some(ActionNarrationPhaseClips {
                    clips: vec![action_narration_clip_for_phase("after")],
                }),
                ..ActionNarration::default()
            },
        }],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "error").await;
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::Error { .. })
    })
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "After narration must not start after Action failure"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
