use super::*;

#[tokio::test]
async fn during_narration_starts_the_action_and_its_tail_gates_the_next_action() {
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
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "During narration must start before its Action is dispatched"
    );

    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();
    let first = receive_action(&mut socket).await;
    assert_eq!(first.payload["text"], "first");
    send_ack(&mut socket, &first.id, "done").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "the next Action must wait for the During Clip tail"
    );

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
async fn during_phase_serializes_every_clip_before_advancing_the_acknowledged_action() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: insert_text("first"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    during: Some(ActionNarrationPhaseClips {
                        clips: vec![
                            action_narration_clip_for_phase("during-first"),
                            action_narration_clip_for_phase("during-second"),
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
    let first_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    assert_eq!(first_clip.clip_index, 0);
    handle
        .commands
        .narration_started(first_clip.clone())
        .unwrap();
    let first_action = receive_action(&mut socket).await;
    assert_eq!(first_action.payload["text"], "first");

    handle.commands.narration_ended(first_clip.clone()).unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Narrating { clip, .. }
                if clip.phase == ActionNarrationPhase::During
                    && clip.action_index == 0
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
    handle
        .commands
        .narration_started(second_clip.clone())
        .unwrap();

    send_ack(&mut socket, &first_action.id, "done").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "the acknowledged Action must wait for the remaining During clips"
    );

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
async fn final_during_phase_waits_for_its_last_clip_before_step_completion() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: insert_text("only"),
            step_id: "step".to_string(),
            narration: ActionNarration {
                during: Some(ActionNarrationPhaseClips {
                    clips: vec![
                        action_narration_clip_for_phase("during-first"),
                        action_narration_clip_for_phase("during-final"),
                    ],
                }),
                ..ActionNarration::default()
            },
        }],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let first_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle
        .commands
        .narration_started(first_clip.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    handle.commands.narration_ended(first_clip.clone()).unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Narrating { clip, .. }
                if clip.phase == ActionNarrationPhase::During && clip.clip_index == 1
        )
    })
    .await;
    let PlaybackState::Narrating {
        clip: final_clip, ..
    } = playback.borrow().clone()
    else {
        unreachable!();
    };
    send_ack(&mut socket, &action.id, "done").await;
    assert!(
        matches!(&*playback.borrow(), PlaybackState::Narrating { .. }),
        "Step completion must wait until the final Clip really starts"
    );
    handle
        .commands
        .narration_started(final_clip.clone())
        .unwrap();

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                continuation: Some(continuation),
                ..
            } if continuation.clip == final_clip
        )
    })
    .await;
    handle.commands.narration_ended(final_clip.clone()).unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::UnitComplete { .. })
    })
    .await;

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn play_after_final_during_clip_advances_to_the_next_step() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = playback_step("first", "First", vec![insert_text("first")]);
    first.actions[0].narration = ActionNarration {
        during: Some(ActionNarrationPhaseClips {
            clips: vec![
                action_narration_clip_for_phase("during-first"),
                action_narration_clip_for_phase("during-final"),
            ],
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
    let first_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle
        .commands
        .narration_started(first_clip.clone())
        .unwrap();
    let first_action = receive_action(&mut socket).await;
    assert_eq!(first_action.payload["text"], "first");
    send_ack(&mut socket, &first_action.id, "done").await;

    handle.commands.narration_ended(first_clip.clone()).unwrap();
    let final_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 1).await;
    handle
        .commands
        .narration_started(final_clip.clone())
        .unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                continuation: Some(continuation),
                ..
            } if continuation.clip == final_clip
        )
    })
    .await;
    handle.commands.narration_ended(final_clip).unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                position,
                continuation: None,
                ..
            } if position.step_id == "first"
        )
    })
    .await;

    handle.commands.play().unwrap();
    let second_action = tokio::time::timeout(Duration::from_secs(1), receive_action(&mut socket))
        .await
        .expect("Play at the boundary should dispatch the next Step");
    assert_eq!(second_action.payload["text"], "second");
    assert!(matches!(
        &*playback.borrow(),
        PlaybackState::Playing { position, .. } if position.step_id == "second"
    ));

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn during_narration_ending_first_still_waits_for_the_action_ack() {
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
    let first = receive_action(&mut socket).await;
    handle
        .commands
        .narration_ended(narration_clip.clone())
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "the next Action must wait for the Action acknowledgement"
    );

    send_ack(&mut socket, &first.id, "done").await;
    let second = receive_action(&mut socket).await;
    assert_eq!(second.payload["text"], "second");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
