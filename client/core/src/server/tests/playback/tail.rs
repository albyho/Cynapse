use super::*;

/// Narration-tail gating of step and unit completion.

#[tokio::test]
async fn final_during_narration_tail_gates_unit_completion() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: insert_text("only"),
            step_id: "step".to_string(),
            narration: ActionNarration {
                during: Some(ActionNarrationPhaseClips {
                    clips: vec![action_narration_clip_for_phase("during")],
                }),
                ..ActionNarration::default()
            },
        }],
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
    wait_for_playback(&mut playback, playing_at(0, 1)).await;
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
        .narration_ended(narration_clip.clone())
        .unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(state, PlaybackState::UnitComplete { .. })
    })
    .await;

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn ordinary_step_completion_keeps_a_started_narration_tail() {
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

    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::StepComplete {
                    continuation: Some(_),
                    ..
                }
            )
        }),
    )
    .await
    .expect("the started narration tail should cross the Step boundary");
    let state = playback.borrow().clone();
    let PlaybackState::StepComplete {
        position,
        continuation: Some(continuation),
        ..
    } = state
    else {
        unreachable!();
    };
    assert_eq!(position.step_id, "first");
    assert_eq!(continuation.clip, narration_clip);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "the next Step must wait for explicit progression"
    );
    handle
        .commands
        .narration_ended(narration_clip.clone())
        .unwrap();
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
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "ending the tail must not dispatch the next Step by itself"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn final_step_tail_completes_the_unit_after_narration_ends() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![PlaybackAction {
            action: insert_text("only"),
            step_id: "step".to_string(),
            narration: ActionNarration {
                during: Some(ActionNarrationPhaseClips {
                    clips: vec![action_narration_clip_for_phase("during")],
                }),
                ..ActionNarration::default()
            },
        }],
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
        .narration_ended(narration_clip.clone())
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(state, PlaybackState::UnitComplete { .. })
        }),
    )
    .await
    .expect("the final Step should complete the Unit after its narration tail");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn final_step_tail_completes_a_nonfinal_unit_after_narration_ends() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = playback_step_in_unit(
        "first_unit",
        "First Unit",
        "only",
        "Only Step",
        vec![insert_text("only")],
    );
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
            playback_step_in_unit(
                "second_unit",
                "Second Unit",
                "next",
                "Next Step",
                vec![insert_text("next")],
            ),
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

    handle.commands.narration_ended(narration_clip).unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete { position }
                    if position.unit_path == "first_unit"
            )
        }),
    )
    .await
    .expect("the final Step should complete the current Unit");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
