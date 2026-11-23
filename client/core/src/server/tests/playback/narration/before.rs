use super::*;

#[tokio::test]
async fn action_before_narration_waits_for_real_media_end_before_dispatch() {
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
                action: insert_text("after before narration"),
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
    let narration_clip =
        wait_for_narrating(&mut playback, ActionNarrationPhase::Before, 0, 0).await;
    handle
        .commands
        .narration_started(narration_clip.clone())
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "an Action must not be sent when Before narration only starts"
    );

    handle
        .commands
        .narration_ended(narration_clip.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    assert_eq!(action.payload["text"], "after before narration");
    assert!(action.payload.get("narration").is_none());

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn before_phase_plays_all_clips_before_dispatching_the_action() {
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
                action: insert_text("after both clips"),
                step_id: "step".to_string(),
                narration: ActionNarration {
                    before: Some(ActionNarrationPhaseClips {
                        clips: vec![
                            action_narration_clip(),
                            action_narration_clip_for_phase("second"),
                        ],
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
    let first = wait_for_narrating(&mut playback, ActionNarrationPhase::Before, 0, 0).await;
    assert_eq!(first.clip_index, 0);
    handle.commands.narration_started(first.clone()).unwrap();
    handle.commands.narration_ended(first.clone()).unwrap();

    let second_clip = loop {
        if let PlaybackState::Narrating { clip, .. } = &*playback.borrow()
            && clip.phase == ActionNarrationPhase::Before
            && clip.action_index == 0
            && clip.clip_index == 1
        {
            break clip.clone();
        }
        playback.changed().await.unwrap();
    };
    assert_eq!(second_clip.step_run_id, first.step_run_id);
    handle
        .commands
        .narration_started(second_clip.clone())
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "the Action must wait for every Before clip"
    );
    handle
        .commands
        .narration_ended(second_clip.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    assert_eq!(action.payload["text"], "after both clips");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn before_and_during_narration_play_in_order_before_action_dispatch() {
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
    let before_instance =
        wait_for_narrating(&mut playback, ActionNarrationPhase::Before, 0, 0).await;
    handle
        .commands
        .narration_started(before_instance.clone())
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );
    handle
        .commands
        .narration_ended(before_instance.clone())
        .unwrap();

    let during_instance =
        wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "During narration must start before its Action is dispatched"
    );
    handle
        .commands
        .narration_started(during_instance.clone())
        .unwrap();
    let action = receive_action(&mut socket).await;
    assert_eq!(action.payload["text"], "combined");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
