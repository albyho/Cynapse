use super::super::*;
use super::support::*;
use crate::course::{EnvironmentRequirement, TerminalCommandAction};
use crate::protocol::CheckPayload;

#[tokio::test]
async fn sends_workspace_reset_paths_to_the_connected_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle
        .commands
        .workspace_reset(vec!["src".to_string()])
        .unwrap();
    let envelope = receive_envelope(&mut socket).await;

    assert_eq!(envelope.kind, MessageType::WorkspaceReset);
    assert_eq!(envelope.payload["paths"][0], "src");
    send_ack(&mut socket, &envelope.id, "done").await;
    assert!(completion.await.unwrap().is_ok());
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn sends_workspace_reset_without_paths_for_terminal_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle.commands.workspace_reset(Vec::new()).unwrap();
    let envelope = receive_envelope(&mut socket).await;

    assert_eq!(envelope.kind, MessageType::WorkspaceReset);
    assert_eq!(envelope.payload["paths"], serde_json::json!([]));
    send_ack(&mut socket, &envelope.id, "done").await;
    assert!(completion.await.unwrap().is_ok());
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn sends_terminal_cleanup_without_closing_editor_tabs() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle
        .commands
        .terminal_cleanup(vec!["ch/unit".to_string()])
        .unwrap();
    let envelope = receive_envelope(&mut socket).await;

    assert_eq!(envelope.kind, MessageType::WorkspaceReset);
    assert_eq!(envelope.payload["paths"][0], "ch/unit");
    assert_eq!(envelope.payload["close_tabs"], false);
    send_ack(&mut socket, &envelope.id, "done").await;
    assert!(completion.await.unwrap().is_ok());
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn sends_terminal_cleanup_scoped_to_one_session() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle
        .commands
        .terminal_cleanup_sessions(vec!["environment".to_string()])
        .unwrap();
    let envelope = receive_envelope(&mut socket).await;

    assert_eq!(envelope.kind, MessageType::WorkspaceReset);
    // No path scope: an empty paths list would close every managed terminal.
    assert_eq!(envelope.payload["paths"], serde_json::json!([]));
    assert_eq!(
        envelope.payload["session_keys"],
        serde_json::json!(["environment"])
    );
    assert_eq!(envelope.payload["close_tabs"], false);
    send_ack(&mut socket, &envelope.id, "done").await;
    assert!(completion.await.unwrap().is_ok());
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn executes_a_one_off_terminal_action_and_returns_its_output() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle
        .commands
        .execute_action(
            Action::TerminalCommand(TerminalCommandAction {
                session_key: "main".to_string(),
                cwd: ".".to_string(),
                command: "rustc --version".to_string(),
                accepted_exit_codes: vec![0],
                expected_output: Vec::new(),
                timeout_ms: 300_000,
                completion_marker: None,
                new_terminal: false,
            }),
            "ch/unit",
        )
        .unwrap();
    let envelope = receive_envelope(&mut socket).await;

    assert_eq!(envelope.kind, MessageType::Action);
    assert_eq!(envelope.unit_path.as_deref(), Some("ch/unit"));
    assert_eq!(envelope.payload["type"], "TerminalCommand");
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: envelope.id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "done",
                    "exit_code": 0,
                    "output": "rustc 1.87.0\n",
                    "output_truncated": false
                }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    let ack = completion.await.unwrap().unwrap();
    assert_eq!(ack.status, AckStatus::Done);
    assert_eq!(ack.exit_code, Some(0));
    assert_eq!(ack.output.as_deref(), Some("rustc 1.87.0\n"));
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn rejects_a_one_off_action_without_a_unit_path() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle.commands.execute_action(Action::Present, "").unwrap();
    assert!(completion.await.unwrap().is_err());

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn a_dropped_one_off_action_does_not_block_the_next_request() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let first = handle
        .commands
        .execute_action(
            Action::TerminalCommand(TerminalCommandAction {
                session_key: "main".to_string(),
                cwd: ".".to_string(),
                command: "rustc --version".to_string(),
                accepted_exit_codes: vec![0],
                expected_output: Vec::new(),
                timeout_ms: 300_000,
                completion_marker: None,
                new_terminal: false,
            }),
            "ch/unit",
        )
        .unwrap();
    let first_envelope = receive_envelope(&mut socket).await;
    drop(first);

    let second = handle
        .commands
        .execute_action(
            Action::TerminalCommand(TerminalCommandAction {
                session_key: "main".to_string(),
                cwd: ".".to_string(),
                command: "cargo --version".to_string(),
                accepted_exit_codes: vec![0],
                expected_output: Vec::new(),
                timeout_ms: 300_000,
                completion_marker: None,
                new_terminal: false,
            }),
            "ch/unit",
        )
        .unwrap();
    let second_envelope = receive_envelope(&mut socket).await;

    assert_eq!(second_envelope.kind, MessageType::Action);
    assert_eq!(second_envelope.payload["type"], "TerminalCommand");
    assert_ne!(first_envelope.id, second_envelope.id);
    send_ack(&mut socket, &second_envelope.id, "done").await;
    assert!(second.await.unwrap().is_ok());
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn cancelling_a_pending_check_sends_cancel_and_completes_it() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle
        .commands
        .execute_check(CheckPayload {
            kind: "COMMAND".to_string(),
            unit_path: "ch/unit".to_string(),
            session_key: "main".to_string(),
            cwd: ".".to_string(),
            command: "long-running-check".to_string(),
            accepted_exit_codes: vec![0],
            expected_output: Vec::new(),
            timeout_ms: 300_000,
            save_dirty: false,
        })
        .unwrap();
    let check = receive_envelope(&mut socket).await;
    assert_eq!(check.kind, MessageType::Check);

    handle
        .commands
        .cancel_pending_one_off(CancelReason::UserStop)
        .unwrap();
    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.kind, MessageType::Cancel);
    assert_eq!(cancel.payload["target_id"], check.id);
    assert_eq!(cancel.payload["reason"], "user_stop");
    assert!(completion.await.unwrap().is_err());

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn checks_environment_versions_through_the_connected_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = tokio::spawn({
        let commands = handle.commands.clone();
        async move {
            commands
                .check_environment(
                    &[
                        EnvironmentRequirement {
                            id: "rustc".to_string(),
                            command: "rustc --version".to_string(),
                            minimum_version: Some("1.85.0".to_string()),
                            install_hint: "Install Rust".to_string(),
                        },
                        EnvironmentRequirement {
                            id: "cargo".to_string(),
                            command: "cargo --version".to_string(),
                            minimum_version: Some("1.85.0".to_string()),
                            install_hint: "Install Rust".to_string(),
                        },
                    ],
                    "ch/unit",
                    "main",
                    ".",
                )
                .await
        }
    });

    let ensure = receive_envelope(&mut socket).await;
    assert_eq!(ensure.payload["type"], "TerminalEnsure");
    assert_eq!(ensure.unit_path.as_deref(), Some("ch/unit"));
    send_terminal_ack(&mut socket, ensure.id, 0, "").await;

    let rustc = receive_envelope(&mut socket).await;
    assert_eq!(rustc.payload["type"], "TerminalCommand");
    assert_eq!(rustc.unit_path.as_deref(), Some("ch/unit"));
    assert_eq!(rustc.payload["command"], "rustc --version");
    send_terminal_ack(&mut socket, rustc.id, 0, "rustc 1.87.0\n").await;

    let cargo = receive_envelope(&mut socket).await;
    assert_eq!(cargo.payload["type"], "TerminalCommand");
    assert_eq!(cargo.unit_path.as_deref(), Some("ch/unit"));
    assert_eq!(cargo.payload["command"], "cargo --version");
    send_terminal_ack(&mut socket, cargo.id, 0, "cargo 1.84.9\n").await;

    let report = completion.await.unwrap();
    assert!(!report.ok);
    assert!(report.checks[0].ok);
    assert!(!report.checks[1].ok);
    assert!(report.checks[1].message.contains("1.85.0"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), receive_envelope(&mut socket))
            .await
            .is_err(),
        "a failed environment check must keep its terminal"
    );
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn closes_environment_terminal_after_a_successful_check() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = tokio::spawn({
        let commands = handle.commands.clone();
        async move {
            commands
                .check_environment(
                    &[EnvironmentRequirement {
                        id: "rustc".to_string(),
                        command: "rustc --version".to_string(),
                        minimum_version: Some("1.85.0".to_string()),
                        install_hint: "Install Rust".to_string(),
                    }],
                    "ch/unit",
                    "main",
                    ".",
                )
                .await
        }
    });

    let ensure = receive_envelope(&mut socket).await;
    assert_eq!(ensure.payload["type"], "TerminalEnsure");
    assert_eq!(ensure.unit_path.as_deref(), Some("ch/unit"));
    send_terminal_ack(&mut socket, ensure.id, 0, "").await;

    let rustc = receive_envelope(&mut socket).await;
    assert_eq!(rustc.payload["type"], "TerminalCommand");
    assert_eq!(rustc.unit_path.as_deref(), Some("ch/unit"));
    send_terminal_ack(&mut socket, rustc.id, 0, "rustc 1.87.0\n").await;

    let cleanup = tokio::time::timeout(Duration::from_millis(100), receive_envelope(&mut socket))
        .await
        .expect("successful environment check should close its terminal");
    assert_eq!(cleanup.kind, MessageType::WorkspaceReset);
    assert_eq!(cleanup.payload["paths"], serde_json::json!([]));
    assert_eq!(cleanup.payload["close_tabs"], false);
    send_ack(&mut socket, &cleanup.id, "done").await;

    let report = completion.await.unwrap();
    assert!(report.ok);
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn reports_unavailable_shell_integration_before_running_commands() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = tokio::spawn({
        let commands = handle.commands.clone();
        async move {
            commands
                .check_environment(
                    &[EnvironmentRequirement {
                        id: "rustc".to_string(),
                        command: "rustc --version".to_string(),
                        minimum_version: Some("1.85.0".to_string()),
                        install_hint: "Install Rust".to_string(),
                    }],
                    "ch/unit",
                    "main",
                    ".",
                )
                .await
        }
    });

    let ensure = receive_envelope(&mut socket).await;
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: ensure.id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "error",
                    "code": "shell_integration_unavailable",
                    "message": "Enable Shell Integration and retry"
                }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

    let report = completion.await.unwrap();
    assert!(!report.ok);
    assert_eq!(report.checks[0].id, "terminal");
    assert!(report.checks[0].message.contains("Shell Integration"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), receive_envelope(&mut socket))
            .await
            .is_err(),
        "a failed environment check must keep its terminal"
    );
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn each_action_gets_a_fresh_ack_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![Action::Present, Action::Present]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let first = receive_action(&mut socket).await;
    tokio::time::advance(ACTION_ACK_TIMEOUT - Duration::from_secs(1)).await;
    send_ack(&mut socket, &first.id, "done").await;

    let second = receive_action(&mut socket).await;
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Playing { position, .. } if position.action_index == 1
            )
        }),
    )
    .await
    .unwrap();

    // The first Action's deadline has expired, but the second Action must still
    // have its own window.
    tokio::time::advance(ACTION_ACK_TIMEOUT - Duration::from_secs(1)).await;
    assert!(matches!(&*playback.borrow(), PlaybackState::Playing { .. }));

    send_ack(&mut socket, &second.id, "done").await;
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn a_check_sent_during_a_slow_workspace_reset_keeps_its_own_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let reset = handle
        .commands
        .workspace_reset(vec!["src".to_string()])
        .unwrap();
    let reset_envelope = receive_envelope(&mut socket).await;
    assert_eq!(reset_envelope.kind, MessageType::WorkspaceReset);
    tokio::time::advance(ACTION_ACK_TIMEOUT - Duration::from_millis(200)).await;

    let check = handle
        .commands
        .execute_check(CheckPayload {
            kind: "COMMAND".to_string(),
            unit_path: "ch/unit".to_string(),
            session_key: "main".to_string(),
            cwd: ".".to_string(),
            command: "long-running-check".to_string(),
            accepted_exit_codes: vec![0],
            expected_output: Vec::new(),
            timeout_ms: 300_000,
            save_dirty: false,
        })
        .unwrap();
    let check_envelope = receive_envelope(&mut socket).await;
    assert_eq!(check_envelope.kind, MessageType::Check);

    // Past the reset's deadline only the reset times out; the check dispatched
    // later must keep its own full window instead of inheriting the remainder.
    tokio::time::advance(Duration::from_millis(300)).await;
    assert!(reset.await.unwrap().is_err());

    tokio::time::advance(ACTION_ACK_TIMEOUT - Duration::from_millis(500)).await;
    send_ack(&mut socket, &check_envelope.id, "done").await;
    assert!(check.await.unwrap().is_ok());

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn a_retried_workspace_reset_gets_a_fresh_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let abandoned = handle
        .commands
        .workspace_reset(vec!["src".to_string()])
        .unwrap();
    let abandoned_envelope = receive_envelope(&mut socket).await;
    assert_eq!(abandoned_envelope.kind, MessageType::WorkspaceReset);
    drop(abandoned);
    tokio::time::advance(ACTION_ACK_TIMEOUT - Duration::from_millis(200)).await;

    let retried = handle
        .commands
        .workspace_reset(vec!["src".to_string()])
        .unwrap();
    let retried_envelope = receive_envelope(&mut socket).await;
    assert_eq!(retried_envelope.kind, MessageType::WorkspaceReset);

    // The retry must not inherit the abandoned request's remaining window.
    tokio::time::advance(Duration::from_millis(300)).await;
    send_ack(&mut socket, &retried_envelope.id, "done").await;
    assert!(retried.await.unwrap().is_ok());

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn an_unanswered_one_off_action_times_out() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let completion = handle
        .commands
        .execute_action(
            Action::TerminalCommand(TerminalCommandAction {
                session_key: "main".to_string(),
                cwd: ".".to_string(),
                command: "rustc --version".to_string(),
                accepted_exit_codes: vec![0],
                expected_output: Vec::new(),
                timeout_ms: 300_000,
                completion_marker: None,
                new_terminal: false,
            }),
            "ch/unit",
        )
        .unwrap();
    let envelope = receive_envelope(&mut socket).await;
    assert_eq!(envelope.kind, MessageType::Action);

    tokio::time::advance(ACTION_ACK_TIMEOUT + Duration::from_secs(1)).await;
    let error = completion.await.unwrap().unwrap_err();
    assert!(
        error.contains("did not acknowledge the action in time"),
        "{error}"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn play_waits_for_a_pending_one_off_check() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![Action::Present]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let check = handle
        .commands
        .execute_check(CheckPayload {
            kind: "COMMAND".to_string(),
            unit_path: "ch/unit".to_string(),
            session_key: "main".to_string(),
            cwd: ".".to_string(),
            command: "long-running-check".to_string(),
            accepted_exit_codes: vec![0],
            expected_output: Vec::new(),
            timeout_ms: 300_000,
            save_dirty: false,
        })
        .unwrap();
    let check_envelope = receive_envelope(&mut socket).await;
    assert_eq!(check_envelope.kind, MessageType::Check);

    handle.commands.play().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), receive_envelope(&mut socket))
            .await
            .is_err(),
        "Play must not dispatch a playback Action while a check is pending"
    );

    send_ack(&mut socket, &check_envelope.id, "done").await;
    assert!(check.await.unwrap().is_ok());

    handle.commands.play().unwrap();
    let action = tokio::time::timeout(Duration::from_secs(2), receive_action(&mut socket))
        .await
        .expect("Play must dispatch once the check is acknowledged");
    assert_eq!(action.kind, MessageType::Action);

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn play_waits_for_a_live_workspace_reset() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![Action::Present]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let reset = handle
        .commands
        .workspace_reset(vec!["src".to_string()])
        .unwrap();
    let envelope = receive_envelope(&mut socket).await;
    assert_eq!(envelope.kind, MessageType::WorkspaceReset);

    handle.commands.play().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), receive_envelope(&mut socket))
            .await
            .is_err(),
        "Play must not dispatch a playback Action while a workspace reset is pending"
    );

    send_ack(&mut socket, &envelope.id, "done").await;
    assert!(reset.await.unwrap().is_ok());

    handle.commands.play().unwrap();
    let action = tokio::time::timeout(Duration::from_secs(2), receive_action(&mut socket))
        .await
        .expect("Play must dispatch once the workspace reset is acknowledged");
    assert_eq!(action.kind, MessageType::Action);

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn play_reaps_an_abandoned_workspace_reset() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![Action::Present]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let reset = handle
        .commands
        .workspace_reset(vec!["src".to_string()])
        .unwrap();
    let envelope = receive_envelope(&mut socket).await;
    assert_eq!(envelope.kind, MessageType::WorkspaceReset);
    drop(reset);

    handle.commands.play().unwrap();
    let action = tokio::time::timeout(Duration::from_secs(2), receive_action(&mut socket))
        .await
        .expect("Play must not be blocked by an abandoned reset");
    assert_eq!(action.kind, MessageType::Action);

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn an_unanswered_action_times_out() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![Action::Present]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let action = receive_action(&mut socket).await;

    tokio::time::advance(ACTION_ACK_TIMEOUT + Duration::from_secs(1)).await;
    wait_for_playback(
        &mut playback,
        |state| matches!(state, PlaybackState::Error { code, .. } if code == "action_timeout"),
    )
    .await;

    let cancel = receive_envelope(&mut socket).await;
    assert_eq!(cancel.kind, MessageType::Cancel);
    assert_eq!(cancel.payload["target_id"], action.id);
    assert_eq!(cancel.payload["reason"], "user_stop");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
