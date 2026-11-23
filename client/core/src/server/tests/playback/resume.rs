use super::*;

/// Resume, retry, pause, and terminal-failure recovery flows.

#[tokio::test]
async fn recovery_replay_skips_wait_actions_in_the_started_step() {
    let directory = tempfile::tempdir().unwrap();
    let step = playback_step(
        "write",
        "Write",
        vec![
            Action::Wait(crate::course::WaitAction {
                duration_ms: 10_000,
            }),
            Action::CreateFile(CreateFileAction {
                path: "after-wait.txt".to_string(),
                create_parents: true,
            }),
        ],
    );
    let handle = start_server_with_path_and_resume(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![step],
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "write".to_string(),
            step_started: true,
            action_index: Some(0),
            recovery: None,
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = tokio::time::timeout(Duration::from_secs(1), receive_action(&mut socket))
        .await
        .expect("Wait should be skipped during recovery");
    assert_eq!(action.payload["type"], "CreateFile");
    handle.shutdown().await;
}

#[tokio::test]
async fn starting_an_unstarted_step_still_honors_wait_actions() {
    let directory = tempfile::tempdir().unwrap();
    let step = playback_step(
        "write",
        "Write",
        vec![
            Action::Wait(crate::course::WaitAction { duration_ms: 75 }),
            Action::CreateFile(CreateFileAction {
                path: "after-wait.txt".to_string(),
                create_parents: true,
            }),
        ],
    );
    let handle = start_server_with_path_and_resume(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![step],
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "write".to_string(),
            step_started: false,
            action_index: None,
            recovery: None,
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;
    let started = tokio::time::Instant::now();

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    assert_eq!(action.payload["type"], "CreateFile");
    assert!(started.elapsed() >= Duration::from_millis(50));
    handle.shutdown().await;
}

#[tokio::test]
async fn only_the_resumed_step_skips_waits() {
    let directory = tempfile::tempdir().unwrap();
    let steps = vec![
        playback_step(
            "first",
            "First",
            vec![
                Action::Wait(crate::course::WaitAction {
                    duration_ms: 10_000,
                }),
                Action::CreateFile(CreateFileAction {
                    path: "first.txt".to_string(),
                    create_parents: true,
                }),
            ],
        ),
        playback_step(
            "second",
            "Second",
            vec![
                Action::Wait(crate::course::WaitAction { duration_ms: 75 }),
                Action::CreateFile(CreateFileAction {
                    path: "second.txt".to_string(),
                    create_parents: true,
                }),
            ],
        ),
    ];
    let handle = start_server_with_path_and_resume(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        steps,
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "first".to_string(),
            step_started: true,
            action_index: Some(0),
            recovery: None,
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    assert_eq!(first.payload["path"], "first.txt");
    send_ack(&mut socket, &first.id, "done").await;
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete {
                position,
                ..
            } if position.step_id == "first"
        )
    })
    .await;

    let started = tokio::time::Instant::now();
    handle.commands.play().unwrap();
    let second = receive_action(&mut socket).await;
    assert_eq!(second.payload["path"], "second.txt");
    assert!(started.elapsed() >= Duration::from_millis(50));
    handle.shutdown().await;
}

#[tokio::test]
async fn replaces_the_playback_plan_without_reconnecting_the_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let handle = start_server_with_path(
        workspace,
        directory.path().join("runtime.json"),
        vec![playback_step_in_unit(
            "first_unit",
            "First Unit",
            "first",
            "First",
            vec![insert_text("first")],
        )],
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let replacement = handle
        .commands
        .replace_plan(
            vec![playback_step_in_unit(
                "second_unit",
                "Second Unit",
                "second",
                "Second",
                vec![insert_text("second")],
            )],
            Some(PlaybackResume {
                unit_path: "second_unit".to_string(),
                step_id: "second".to_string(),
                step_started: false,
                action_index: None,
                recovery: None,
            }),
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), replacement)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let mut playback = handle.playback.clone();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Ready {
                position: Some(position)
            } if position.unit_path == "second_unit"
        )
    })
    .await;
    assert!(matches!(
        &*handle.state.borrow(),
        ConnectionState::Connected { .. }
    ));

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    let Action::InsertText(action) = serde_json::from_value(action.payload).unwrap() else {
        panic!("expected InsertText");
    };
    assert_eq!(action.text, "second");
}

#[tokio::test]
async fn resuming_a_step_starts_from_its_first_action() {
    let directory = tempfile::tempdir().unwrap();
    let step = playback_step(
        "write",
        "Write",
        vec![insert_text("first"), insert_text("second")],
    );
    let handle = start_server_with_path_and_resume(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![step],
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "write".to_string(),
            step_started: true,
            action_index: None,
            recovery: None,
        }),
    )
    .await
    .unwrap();

    let playback = handle.playback.borrow().clone();
    let PlaybackState::Ready {
        position: Some(position),
    } = playback
    else {
        panic!("expected ready playback");
    };
    assert_eq!(position.step_id, "write");
    assert!(position.step_started);
    assert_eq!(position.action_index, 0);

    handle.shutdown().await;
}

#[tokio::test]
async fn restores_ready_at_saved_progress_without_sending_an_action() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let steps = vec![
        playback_step("first", "First Step", vec![insert_text("first")]),
        playback_step("second", "Second Step", vec![insert_text("second")]),
    ];
    let handle = start_server_with_path_and_resume(
        workspace,
        directory.path().join("runtime.json"),
        steps,
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "second".to_string(),
            step_started: false,
            action_index: None,
            recovery: None,
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let playback = handle.playback.borrow().clone();
    let PlaybackState::Ready {
        position: Some(position),
    } = playback
    else {
        panic!("expected ready position");
    };
    assert_eq!(position.step_id, "second");
    assert_eq!(position.action_index, 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn restores_ready_at_the_start_of_a_step() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let handle = start_server_with_path_and_resume(
        workspace,
        directory.path().join("runtime.json"),
        vec![playback_step(
            "first",
            "First Step",
            vec![insert_text("first"), insert_text("second")],
        )],
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "first".to_string(),
            step_started: false,
            action_index: None,
            recovery: None,
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let playback = handle.playback.borrow().clone();
    let PlaybackState::Ready {
        position: Some(position),
    } = playback
    else {
        panic!("expected ready position");
    };
    assert_eq!(position.step_id, "first");
    assert_eq!(position.action_index, 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn persisted_forced_termination_restores_as_rerunnable_pause() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let handle = start_server_with_path_and_resume(
        workspace,
        directory.path().join("runtime.json"),
        vec![playback_step(
            "first",
            "First Step",
            vec![Action::TerminalCommand(TerminalCommandAction {
                session_key: "main".to_string(),
                cwd: "hello_world".to_string(),
                command: "rustc main.rs".to_string(),
                accepted_exit_codes: vec![0],
                expected_output: Vec::new(),
                timeout_ms: 300_000,
                completion_marker: None,
                new_terminal: false,
            })],
        )],
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "first".to_string(),
            step_started: false,
            action_index: None,
            recovery: Some(PlaybackRecovery {
                reason: PlaybackInterruptionReason::ForcedTermination,
                command: "rustc main.rs".to_string(),
                output: None,
                output_truncated: false,
                exit_code: None,
            }),
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    assert!(matches!(
        handle.playback.borrow().clone(),
        PlaybackState::Paused {
            interruption: PlaybackInterruption { reason, .. },
            ..
        } if reason == PlaybackInterruptionReason::ForcedTermination
    ));

    handle.commands.play().unwrap();
    let rerun = receive_action(&mut socket).await;
    assert_eq!(rerun.payload["command"], "rustc main.rs");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn plays_filesystem_action_primitives_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let actions = vec![
        Action::CreateFile(CreateFileAction {
            path: "lesson/hello.txt".to_string(),
            create_parents: true,
        }),
        Action::DeleteFile(DeleteFileAction {
            path: "lesson/hello.txt".to_string(),
            if_missing: MissingTargetPolicy::Ignore,
        }),
    ];
    let handle = start_test_server(directory.path(), actions).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let create = receive_action(&mut socket).await;
    assert_eq!(create.payload["type"], "CreateFile");
    assert_eq!(create.payload["path"], "lesson/hello.txt");
    send_ack(&mut socket, &create.id, "done").await;

    let delete = receive_action(&mut socket).await;
    assert_eq!(delete.payload["type"], "DeleteFile");
    assert_eq!(delete.payload["if_missing"], "ignore");
    send_ack(&mut socket, &delete.id, "done").await;

    let mut playback = handle.playback.clone();
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
async fn pauses_and_resumes_a_filesystem_primitive() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(
        directory.path(),
        vec![Action::CreateDirectory(CreateDirectoryAction {
            path: "lesson".to_string(),
            create_parents: true,
        })],
    )
    .await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let create = receive_action(&mut socket).await;
    handle.commands.cancel().unwrap();
    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.kind, MessageType::Cancel);
    assert_eq!(cancel.payload["target_id"], create.id);
    send_ack(&mut socket, &create.id, "cancelled").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, paused_at(0, 1)),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    let resumed = receive_action(&mut socket).await;
    assert_eq!(resumed.payload["type"], "CreateDirectory");
    send_ack(&mut socket, &resumed.id, "done").await;

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn retrying_after_pause_restarts_the_step_from_its_first_action() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(
        directory.path(),
        vec![
            insert_text("first"),
            insert_text("second"),
            insert_text("third"),
        ],
    )
    .await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let mut playback = handle.playback.clone();
    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    send_ack(&mut socket, &first.id, "done").await;

    let second = receive_action(&mut socket).await;
    handle.commands.cancel().unwrap();
    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.kind, MessageType::Cancel);
    assert_eq!(cancel.payload["target_id"], second.id);
    assert_eq!(cancel.payload["reason"], "user_stop");

    send_ack(&mut socket, &second.id, "cancelled").await;
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, paused_at(1, 3)),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    let resumed = receive_action(&mut socket).await;
    assert_eq!(resumed.payload["text"], "first");
    send_ack(&mut socket, &resumed.id, "done").await;

    let second = receive_action(&mut socket).await;
    assert_eq!(second.payload["text"], "second");
    send_ack(&mut socket, &second.id, "done").await;

    let third = receive_action(&mut socket).await;
    send_ack(&mut socket, &third.id, "done").await;
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, unit_complete_at(3, 3)),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn course_switch_cancel_preserves_partial_output_for_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(
        directory.path(),
        vec![Action::TerminalCommand(TerminalCommandAction {
            session_key: "main".to_string(),
            cwd: "hello_world".to_string(),
            command: "rustc main.rs".to_string(),
            accepted_exit_codes: vec![0],
            expected_output: Vec::new(),
            timeout_ms: 300_000,
            completion_marker: None,
            new_terminal: false,
        })],
    )
    .await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    handle
        .commands
        .cancel_with_reason(CancelReason::CourseSwitch)
        .unwrap();
    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.payload["reason"], "course_switch");
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: action.id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "cancelled",
                    "output": "partial compiler output",
                    "output_truncated": false
                }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Paused {
                    interruption: PlaybackInterruption {
                        reason,
                        output: Some(output),
                        ..
                    },
                    ..
                } if *reason == PlaybackInterruptionReason::CourseSwitch
                    && output == "partial compiler output"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn one_play_restarts_a_paused_step_from_its_first_action() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(
        directory.path(),
        vec![insert_text("first"), insert_text("last")],
    )
    .await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    send_ack(&mut socket, &first.id, "done").await;

    let last = receive_action(&mut socket).await;
    handle.commands.cancel().unwrap();
    let _ = receive_envelope(&mut socket).await;
    send_ack(&mut socket, &last.id, "cancelled").await;

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, paused_at(1, 2)),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    let resumed = tokio::time::timeout(Duration::from_secs(1), receive_action(&mut socket))
        .await
        .expect("one Play should restart the paused step");
    assert_eq!(resumed.payload["text"], "first");
    send_ack(&mut socket, &resumed.id, "done").await;

    let retried_last = receive_action(&mut socket).await;
    assert_eq!(retried_last.payload["text"], "last");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn unexpected_terminal_failure_stays_on_the_command_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let command = Action::TerminalCommand(TerminalCommandAction {
        session_key: "main".to_string(),
        cwd: "hello_world".to_string(),
        command: "rustc main.rs".to_string(),
        accepted_exit_codes: vec![0],
        expected_output: Vec::new(),
        timeout_ms: 300_000,
        completion_marker: None,
        new_terminal: false,
    });
    let handle = start_test_server(directory.path(), vec![command]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: first.id.clone(),
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "error",
                    "code": "terminal_exit_code_rejected",
                    "message": "Command exited with code 1",
                    "exit_code": 1,
                    "output": "error[E0384]: cannot assign twice to immutable variable\n"
                }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Error {
                    code,
                    position: Some(PlaybackPosition {
                        action_index: 0,
                        action_total: 1,
                        ..
                    }),
                    ..
                } if code == "terminal_exit_code_rejected"
            )
        }),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    let retry = receive_action(&mut socket).await;
    assert_eq!(retry.payload["type"], "TerminalCommand");
    assert_ne!(retry.id, first.id);
    assert_eq!(retry.payload["command"], "rustc main.rs");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn retrying_a_failed_step_restarts_from_its_first_action() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![
            PlaybackAction {
                action: insert_text("first"),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
            PlaybackAction {
                action: insert_text("second"),
                step_id: "step".to_string(),
                narration: ActionNarration::default(),
            },
        ],
    )
    .await;

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    send_ack(&mut socket, &first.id, "done").await;
    let second = receive_action(&mut socket).await;
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: second.id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "error",
                    "code": "apply_failed",
                    "message": "save failed"
                }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Error {
                    position: Some(PlaybackPosition {
                        action_index: 1,
                        action_total: 2,
                        ..
                    }),
                    ..
                }
            )
        }),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    let retry = receive_action(&mut socket).await;
    assert_eq!(retry.payload["text"], "first");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn terminal_timeout_enters_retryable_state_with_partial_output() {
    let directory = tempfile::tempdir().unwrap();
    let command = Action::TerminalCommand(TerminalCommandAction {
        session_key: "main".to_string(),
        cwd: "hello_world".to_string(),
        command: "slow-command".to_string(),
        accepted_exit_codes: vec![0],
        expected_output: Vec::new(),
        timeout_ms: 50,
        completion_marker: None,
        new_terminal: false,
    });
    let handle = start_test_server(directory.path(), vec![command]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: action.id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "cancelled",
                    "code": "terminal_timeout",
                    "timed_out": true,
                    "message": "Command timed out after 50 ms",
                    "output": "partial output",
                    "output_truncated": false
                }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Error {
                    code,
                    interruption: Some(PlaybackInterruption {
                        reason,
                        output: Some(output),
                        ..
                    }),
                    ..
                } if code == "terminal_timeout"
                    && *reason == PlaybackInterruptionReason::Timeout
                    && output == "partial output"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn adapter_disconnect_pauses_the_running_command_for_rerun() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(
        directory.path(),
        vec![Action::TerminalCommand(TerminalCommandAction {
            session_key: "main".to_string(),
            cwd: "hello_world".to_string(),
            command: "rustc main.rs".to_string(),
            accepted_exit_codes: vec![0],
            expected_output: Vec::new(),
            timeout_ms: 300_000,
            completion_marker: None,
            new_terminal: false,
        })],
    )
    .await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    socket.close(None).await.unwrap();

    let mut playback = handle.playback.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Paused {
                    interruption: PlaybackInterruption { reason, .. },
                    ..
                } if *reason == PlaybackInterruptionReason::AdapterDisconnected
            )
        }),
    )
    .await
    .unwrap();

    let (mut reconnected, discovery) = connect(&handle).await;
    send_hello(&mut reconnected, discovery.token).await;
    assert!(matches!(
        handle.playback.borrow().clone(),
        PlaybackState::Paused {
            interruption: PlaybackInterruption { reason, .. },
            ..
        } if reason == PlaybackInterruptionReason::AdapterDisconnected
    ));

    handle.commands.play().unwrap();
    let rerun = receive_action(&mut reconnected).await;
    assert_eq!(rerun.payload["command"], "rustc main.rs");
    assert_ne!(rerun.id, first.id);

    reconnected.close(None).await.unwrap();
    handle.shutdown().await;
}
