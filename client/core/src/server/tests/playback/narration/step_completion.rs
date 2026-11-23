use super::*;

#[tokio::test]
async fn ordinary_step_completion_waits_for_after_narration_to_start() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = playback_step("first", "First", vec![insert_text("first")]);
    first.actions[0].narration = ActionNarration {
        after: Some(ActionNarrationPhaseClips {
            clips: vec![action_narration_clip_for_phase("after")],
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
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;
    let narration_clip = wait_for_narrating(&mut playback, ActionNarrationPhase::After, 0, 0).await;
    assert!(
        !matches!(
            &*playback.borrow(),
            PlaybackState::StepComplete {
                continuation: Some(_),
                ..
            }
        ),
        "the After Clip must really start before Step completion"
    );
    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();

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
    .expect("the started After Clip should cross the Step boundary");
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

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn human_step_waits_for_action_narration_to_end() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = playback_step("first", "First", vec![insert_text("first")]);
    first.requires_human = true;
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
            PlaybackState::Narrating { clip, .. }
                if clip.phase == ActionNarrationPhase::During
                    && clip.action_index == 0
                    && clip.clip_index == 0
        )
    })
    .await;

    handle
        .commands
        .narration_ended(narration_clip.clone())
        .unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                requires_human: true,
                continuation: None,
                ..
            }
        )
    })
    .await;

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn final_human_step_waits_for_narration_then_completes_the_unit() {
    let directory = tempfile::tempdir().unwrap();
    let mut step = playback_step("human", "Human", vec![insert_text("human")]);
    step.requires_human = true;
    step.actions[0].narration = ActionNarration {
        during: Some(ActionNarrationPhaseClips {
            clips: vec![action_narration_clip_for_phase("during")],
        }),
        ..ActionNarration::default()
    };
    let handle = start_server_with_path(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![step],
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
    assert!(
        !matches!(&*playback.borrow(), PlaybackState::StepComplete { .. }),
        "a final human Step must wait for its narration tail"
    );
    handle.commands.narration_ended(narration_clip).unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(state, PlaybackState::UnitComplete { .. })
        }),
    )
    .await
    .expect("a final human Step must complete the Unit after its narration tail");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
