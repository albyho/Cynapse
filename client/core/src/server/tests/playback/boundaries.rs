use super::*;
use cynapse_client_i18n::t_with;

/// Step, unit, and node boundary stops with explicit play.

#[tokio::test]
async fn plays_two_actions_in_order_and_stops_at_node_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let actions = vec![insert_text("first"), insert_text("second")];
    let handle = start_test_server(directory.path(), actions).await;
    let (mut socket, discovery) = connect(&handle).await;

    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, ready_at(0, 2, 2)),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    let first = socket.next().await.unwrap().unwrap();
    let Message::Text(first) = first else {
        panic!("expected first action");
    };
    let first = Envelope::parse(first.as_str()).unwrap();
    assert_eq!(first.kind, MessageType::Action);
    let first_id = first.id.clone();

    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, playing_at(0, 2)),
    )
    .await
    .unwrap();
    let PlaybackState::Playing { action_label, .. } = playback.borrow().clone() else {
        panic!("expected playing state");
    };
    assert_eq!(
        action_label,
        t_with("action.insert_text", &[("path", "src/main.rs")])
    );

    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: first_id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({ "status": "done" }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    let second = socket.next().await.unwrap().unwrap();
    let Message::Text(second) = second else {
        panic!("expected second action");
    };
    let second = Envelope::parse(second.as_str()).unwrap();
    assert_eq!(second.kind, MessageType::Action);
    let second_id = second.id.clone();

    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: second_id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({ "status": "done" }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, unit_complete_at(2, 2)),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn stops_at_step_boundaries_and_waits_for_explicit_play() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let steps = vec![
        playback_step("first", "First Step", vec![insert_text("first")]),
        playback_step("second", "Second Step", vec![insert_text("second")]),
    ];
    let handle = start_server_with_path(workspace, directory.path().join("runtime.json"), steps)
        .await
        .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let mut playback = handle.playback.clone();
    let ready = playback.borrow().clone();
    let PlaybackState::Ready {
        position: Some(position),
    } = ready
    else {
        panic!("expected ready position");
    };
    assert_eq!(position.parent_title, "Step Grouping");
    assert_eq!(position.step_title, "First Step");
    assert_eq!(position.overall_action_total, 2);

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    assert_eq!(first.payload["text"], "first");
    send_ack(&mut socket, &first.id, "done").await;

    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::StepComplete {
                    position: PlaybackPosition {
                        step_index: 0,
                        step_total: 2,
                        overall_action_index: 1,
                        overall_action_total: 2,
                        ..
                    },
                    ..
                }
            )
        }),
    )
    .await
    .unwrap();

    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );

    handle.commands.play().unwrap();
    let second = receive_action(&mut socket).await;
    assert_eq!(second.payload["text"], "second");
    send_ack(&mut socket, &second.id, "done").await;

    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete {
                    position: PlaybackPosition {
                        step_index: 1,
                        step_total: 2,
                        overall_action_index: 2,
                        overall_action_total: 2,
                        ..
                    }
                }
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn stops_at_unit_boundaries_and_waits_for_explicit_play() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let steps = vec![
        playback_step_in_unit(
            "first_unit",
            "First Unit",
            "shared",
            "Shared Step",
            vec![insert_text("first")],
        ),
        playback_step_in_unit(
            "second_unit",
            "Second Unit",
            "shared",
            "Shared Step",
            vec![insert_text("second")],
        ),
    ];
    let handle = start_server_with_path(workspace, directory.path().join("runtime.json"), steps)
        .await
        .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    send_ack(&mut socket, &first.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete {
                    position: PlaybackPosition {
                        unit_path,
                        step_id,
                        ..
                    }
                } if unit_path == "first_unit" && step_id == "shared"
            )
        }),
    )
    .await
    .unwrap();

    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );

    handle.commands.play().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "a completed Unit must not auto-start the next Unit"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn stops_before_node_completion_when_other_units_remain() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let steps = vec![playback_step_in_unit(
        "unit_e",
        "Unit E",
        "shared",
        "Shared Step",
        vec![insert_text("independent")],
    )];
    let handle = start_server_with_path_and_resume(
        workspace,
        directory.path().join("runtime.json"),
        steps,
        Some(PlaybackResume {
            unit_path: "unit_e".to_string(),
            step_id: "shared".to_string(),
            step_started: false,
            action_index: None,
            recovery: None,
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete {
                    position: PlaybackPosition {
                        unit_path,
                        ..
                    }
                } if unit_path == "unit_e"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn continues_within_a_step_before_stopping_at_its_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let steps = vec![
        playback_step(
            "first",
            "First Step",
            vec![insert_text("first"), insert_text("continued")],
        ),
        playback_step("second", "Second Step", vec![insert_text("second")]),
    ];
    let handle = start_server_with_path(workspace, directory.path().join("runtime.json"), steps)
        .await
        .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    send_ack(&mut socket, &first.id, "done").await;

    let continued = receive_action(&mut socket).await;
    assert_eq!(continued.payload["text"], "continued");
    send_ack(&mut socket, &continued.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::StepComplete {
                    position: PlaybackPosition {
                        step_id,
                        action_index: 2,
                        action_total: 2,
                        overall_action_index: 2,
                        overall_action_total: 3,
                        ..
                    },
                    ..
                } if step_id == "first"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn never_starts_the_next_unit_without_explicit_navigation() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let steps = vec![
        PlaybackStep {
            parent_id: "ch_01".to_string(),
            parent_title: "First Grouping".to_string(),
            unit_path: "unit_01".to_string(),
            unit_title: "First Unit".to_string(),
            step_id: "01_01".to_string(),
            step_title: "First Grouping Step".to_string(),
            requires_human: false,
            challenge: false,
            actions: vec![crate::course::PlaybackAction {
                action: insert_text("chapter one"),
                step_id: "01_01".to_string(),
                narration: ActionNarration::default(),
            }],
        },
        PlaybackStep {
            parent_id: "ch_02".to_string(),
            parent_title: "Second Grouping".to_string(),
            unit_path: "unit_02".to_string(),
            unit_title: "Second Unit".to_string(),
            step_id: "02_01".to_string(),
            step_title: "Second Grouping Step".to_string(),
            requires_human: false,
            challenge: false,
            actions: vec![crate::course::PlaybackAction {
                action: insert_text("chapter two"),
                step_id: "02_01".to_string(),
                narration: ActionNarration::default(),
            }],
        },
    ];
    let handle = start_server_with_path(workspace, directory.path().join("runtime.json"), steps)
        .await
        .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    assert_eq!(first.payload["text"], "chapter one");
    send_ack(&mut socket, &first.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete {
                    position: PlaybackPosition {
                        parent_id,
                        parent_title,
                        ..
                    }
                } if parent_id == "ch_01" && parent_title == "First Grouping"
            )
        }),
    )
    .await
    .unwrap();

    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );

    handle.commands.play().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "a completed unit must wait for explicit navigation"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn a_human_final_step_completes_the_unit_without_an_explicit_advance() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut human = playback_step_in_unit(
        "first_unit",
        "First Unit",
        "human",
        "Human Step",
        vec![insert_text("human")],
    );
    human.requires_human = true;
    let steps = vec![
        human,
        playback_step_in_unit(
            "second_unit",
            "Second Unit",
            "next",
            "Next Step",
            vec![insert_text("next")],
        ),
    ];
    let handle = start_server_with_path(workspace, directory.path().join("runtime.json"), steps)
        .await
        .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete {
                    position: PlaybackPosition {
                        unit_path,
                        step_id,
                        ..
                    }
                } if unit_path == "first_unit" && step_id == "human"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
#[tokio::test]
async fn a_human_step_in_the_last_unit_completes_the_unit_without_an_explicit_advance() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut human = playback_step("human", "Human Step", vec![insert_text("human")]);
    human.requires_human = true;
    let handle = start_server_with_path(
        workspace,
        directory.path().join("runtime.json"),
        vec![human],
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete { position }
                    if position.unit_path == "main_unit"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn a_final_challenge_completes_the_unit_through_the_normal_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut challenge = playback_step_in_unit(
        "only_unit",
        "Only Unit",
        "challenge",
        "Final Challenge",
        vec![insert_text("challenge")],
    );
    challenge.challenge = true;
    let handle = start_server_with_path(
        workspace,
        directory.path().join("runtime.json"),
        vec![challenge],
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::StepComplete {
                    position,
                    requires_human: true,
                    ..
                } if position.unit_path == "only_unit" && position.step_id == "challenge"
            )
        }),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete { position }
                    if position.unit_path == "only_unit" && position.step_id == "challenge"
            )
        }),
    )
    .await
    .expect("the final Challenge must end at UnitComplete");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn a_reconnected_adapter_can_finish_a_final_challenge_unit() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut challenge = playback_step_in_unit(
        "only_unit",
        "Only Unit",
        "challenge",
        "Final Challenge",
        vec![insert_text("challenge")],
    );
    challenge.challenge = true;
    let handle = start_server_with_path(
        workspace,
        directory.path().join("runtime.json"),
        vec![challenge],
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::StepComplete {
                    requires_human: true,
                    ..
                }
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    let (mut reconnected, discovery) = connect(&handle).await;
    send_hello(&mut reconnected, discovery.token).await;

    handle.commands.play().unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete { position }
                    if position.unit_path == "only_unit" && position.step_id == "challenge"
            )
        }),
    )
    .await
    .expect("a reconnected adapter must consume the stored Challenge boundary");

    reconnected.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn a_challenge_final_step_waits_for_its_checkpoint_before_completing_the_unit() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut challenge = playback_step_in_unit(
        "first_unit",
        "First Unit",
        "challenge",
        "Challenge Step",
        vec![insert_text("challenge")],
    );
    challenge.challenge = true;
    let steps = vec![
        challenge,
        playback_step_in_unit(
            "second_unit",
            "Second Unit",
            "next",
            "Next Step",
            vec![insert_text("next")],
        ),
    ];
    let handle = start_server_with_path(workspace, directory.path().join("runtime.json"), steps)
        .await
        .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    send_ack(&mut socket, &action.id, "done").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::StepComplete {
                    position: PlaybackPosition {
                        unit_path,
                        step_id,
                        ..
                    },
                    requires_human: true,
                    ..
                } if unit_path == "first_unit" && step_id == "challenge"
            )
        }),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete {
                    position: PlaybackPosition {
                        unit_path,
                        step_id,
                        ..
                    }
                } if unit_path == "first_unit" && step_id == "challenge"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
