use super::*;

#[tokio::test]
async fn after_narration_starts_only_after_successful_action_ack() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: insert_text("first"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    after: Some(ActionNarrationPhaseClips {
                        clips: vec![action_narration_clip_for_phase("after")],
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
    let first = receive_action(&mut socket).await;
    send_ack(&mut socket, &first.id, "done").await;
    let narration_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::After, 0, 0).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "the next Action must wait for the After Clip"
    );

    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();
    handle
        .commands
        .narration_ended(narration_clip.clone())
        .unwrap();
    let second = receive_action(&mut socket).await;
    assert_eq!(second.payload["text"], "second");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn after_phase_plays_every_clip_after_the_action_acknowledgement() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: insert_text("first"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    after: Some(ActionNarrationPhaseClips {
                        clips: vec![
                            action_narration_clip_for_phase("after-first"),
                            action_narration_clip_for_phase("after-second"),
                        ],
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
    let first_action = receive_action(&mut socket).await;
    assert_eq!(first_action.payload["text"], "first");
    send_ack(&mut socket, &first_action.id, "done").await;

    let first_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::After, 0, 0).await;
    handle
        .commands
        .narration_started(first_clip.clone())
        .unwrap();
    handle.commands.narration_ended(first_clip.clone()).unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Narrating { clip, .. }
                if clip.phase == ActionNarrationPhase::After
                    && clip.clip_index == 1
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
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "the next Action must wait for every After clip"
    );
    handle
        .commands
        .narration_started(second_clip.clone())
        .unwrap();
    handle
        .commands
        .narration_ended(second_clip.clone())
        .unwrap();
    let second_action = receive_action(&mut socket).await;
    assert_eq!(second_action.payload["text"], "second");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn before_and_after_narration_release_before_then_start_after_ack() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: insert_text("combined"),
            step_id: "step".to_string(),
            narration: ActionNarration {
                before: Some(ActionNarrationPhaseClips {
                    clips: vec![action_narration_clip_for_phase("before")],
                }),
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
    let before_instance =
        wait_for_narrating(&mut playback, ActionNarrationPhase::Before, 0, 0).await;
    handle
        .commands
        .narration_started(before_instance.clone())
        .unwrap();
    handle
        .commands
        .narration_ended(before_instance.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;

    let after_instance = wait_for_narrating(&mut playback, ActionNarrationPhase::After, 0, 0).await;
    handle
        .commands
        .narration_started(after_instance.clone())
        .unwrap();
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
        .narration_ended(after_instance.clone())
        .unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::UnitComplete { .. })
    })
    .await;

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
