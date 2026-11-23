use super::super::*;
use super::support::*;

#[tokio::test]
async fn accepts_valid_hello_and_returns_ack() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    assert!(handle.runtime_path.exists());
    let runtime_path = handle.runtime_path.clone();

    let (mut socket, discovery) = connect(&handle).await;
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token.clone())
                .to_json()
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();

    let reply = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = reply else {
        panic!("expected hello_ack text");
    };
    let envelope = Envelope::parse(text.as_str()).unwrap();
    assert_eq!(envelope.kind, MessageType::HelloAck);
    assert_eq!(envelope.v, PROTOCOL_VERSION);

    let mut state = handle.state.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(state, ConnectionState::Connected { .. })
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(state, ConnectionState::WaitingForAdapter)
        }),
    )
    .await
    .unwrap();

    handle.shutdown().await;
    assert!(!runtime_path.exists());
}

#[tokio::test]
async fn binds_the_configured_control_port() {
    let directory = tempfile::tempdir().unwrap();
    let port = free_loopback_port();
    let handle =
        start_test_server_for_adapter_on_port(directory.path(), crate::AdapterId::Jupyterlab, port)
            .await;

    assert_eq!(handle.port, port);
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    socket.close(None).await.ok();
    handle.shutdown().await;
}

#[tokio::test]
async fn a_busy_control_port_is_an_actionable_error() {
    let directory = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::fs::create_dir_all(directory.path().join("workspace")).unwrap();

    let error = match start_server_with_path_and_resume_for_adapter_on_port(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![playback_step("main", "Main Step", Vec::new())],
        None,
        AdapterTarget::External(crate::AdapterId::Jupyterlab),
        port,
    )
    .await
    {
        Ok(_) => panic!("the busy control port must fail the server start"),
        Err(error) => error,
    };

    assert!(matches!(error, ServerError::PortInUse { port: actual } if actual == port));
    assert!(
        error.to_string().contains(&port.to_string()),
        "the error must name the busy port: {error}"
    );
    drop(listener);
}

#[tokio::test]
async fn a_newer_adapter_connection_replaces_the_older_one() {
    let directory = tempfile::tempdir().unwrap();
    let handle =
        start_test_server_for_adapter(directory.path(), crate::AdapterId::Jupyterlab).await;
    let mut state = handle.state.clone();

    let (mut older, discovery) = connect(&handle).await;
    send_hello(&mut older, discovery.token.clone()).await;
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(state, ConnectionState::Connected { .. })
        }),
    )
    .await
    .unwrap();

    let (mut newer, _) = connect(&handle).await;
    send_hello(&mut newer, discovery.token).await;

    let close = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match older.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return None,
            }
        }
    })
    .await
    .expect("the older page must be told it is inactive");
    let frame = close.expect("the older page must receive a close frame");
    assert_eq!(
        u16::from(frame.code),
        crate::server::INACTIVE_ADAPTER_CLOSE_CODE
    );
    assert!(
        frame.reason.to_lowercase().contains("newer"),
        "close reason must explain that a newer page is active: {}",
        frame.reason
    );
    assert!(matches!(
        &*state.borrow(),
        ConnectionState::Connected { adapter } if adapter == "jupyterlab"
    ));

    newer.close(None).await.ok();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(state, ConnectionState::WaitingForAdapter)
        }),
    )
    .await
    .unwrap();

    handle.shutdown().await;
}

pub(super) fn free_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn reports_the_course_adapter_in_the_handshake() {
    let directory = tempfile::tempdir().unwrap();
    let handle =
        start_test_server_for_adapter(directory.path(), crate::AdapterId::Jupyterlab).await;

    let (mut socket, discovery) = connect(&handle).await;
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();

    let envelope = receive_envelope(&mut socket).await;
    assert_eq!(envelope.kind, MessageType::HelloAck);
    let payload = serde_json::from_value::<crate::HelloAckPayload>(envelope.payload).unwrap();
    assert_eq!(payload.adapter, "jupyterlab");

    let mut state = handle.state.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(
                state,
                ConnectionState::Connected { adapter } if adapter == "jupyterlab"
            )
        }),
    )
    .await
    .unwrap();

    handle.shutdown().await;
}

#[tokio::test]
async fn rejects_wrong_token() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, _) = connect(&handle).await;

    socket
        .send(Message::Text(
            Envelope::hello("wrong-token").to_json().unwrap().into(),
        ))
        .await
        .unwrap();

    let reply = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = reply else {
        panic!("expected ack text");
    };
    let envelope = Envelope::parse(text.as_str()).unwrap();
    let payload = serde_json::from_value::<AckPayload>(envelope.payload).unwrap();

    assert_eq!(payload.status, AckStatus::Error);
    assert_eq!(payload.code.as_deref(), Some("token_mismatch"));

    handle.shutdown().await;
}

const JUPYTERLAB_ORIGIN: &str = "http://127.0.0.1:18890";

async fn expect_rejection(socket: &mut ClientSocket) -> AckPayload {
    let reply = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = reply else {
        panic!("expected ack text");
    };
    let envelope = Envelope::parse(text.as_str()).unwrap();
    let payload = serde_json::from_value::<AckPayload>(envelope.payload).unwrap();
    assert_eq!(payload.status, AckStatus::Error);
    payload
}

#[tokio::test]
async fn accepts_the_launched_jupyterlab_origin_without_a_protocol_token() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server_for_jupyterlab(directory.path(), JUPYTERLAB_ORIGIN).await;
    let (mut socket, _) = connect_with_origin(&handle, Some(JUPYTERLAB_ORIGIN)).await;

    socket
        .send(Message::Text(
            Envelope::hello(String::new()).to_json().unwrap().into(),
        ))
        .await
        .unwrap();

    let envelope = receive_envelope(&mut socket).await;
    assert_eq!(envelope.kind, MessageType::HelloAck);
    let payload = serde_json::from_value::<crate::HelloAckPayload>(envelope.payload).unwrap();
    assert_eq!(payload.adapter, "jupyterlab");

    let mut state = handle.state.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(
                state,
                ConnectionState::Connected { adapter } if adapter == "jupyterlab"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.ok();
    handle.shutdown().await;
}

#[tokio::test]
async fn rejects_a_different_jupyterlab_origin() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server_for_jupyterlab(directory.path(), JUPYTERLAB_ORIGIN).await;
    let (mut socket, _) = connect_with_origin(&handle, Some("http://127.0.0.1:18891")).await;

    socket
        .send(Message::Text(
            Envelope::hello(String::new()).to_json().unwrap().into(),
        ))
        .await
        .unwrap();

    let payload = expect_rejection(&mut socket).await;
    assert_eq!(payload.code.as_deref(), Some("invalid_origin"));
    assert!(matches!(
        &*handle.state.borrow(),
        ConnectionState::Error { code, .. } if code == "invalid_origin"
    ));

    handle.shutdown().await;
}

#[tokio::test]
async fn rejects_a_missing_jupyterlab_origin() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server_for_jupyterlab(directory.path(), JUPYTERLAB_ORIGIN).await;
    let (mut socket, _) = connect_with_origin(&handle, None).await;

    socket
        .send(Message::Text(
            Envelope::hello(String::new()).to_json().unwrap().into(),
        ))
        .await
        .unwrap();

    let payload = expect_rejection(&mut socket).await;
    assert_eq!(payload.code.as_deref(), Some("invalid_origin"));
    assert!(matches!(
        &*handle.state.borrow(),
        ConnectionState::Error { code, .. } if code == "invalid_origin"
    ));

    handle.shutdown().await;
}

#[tokio::test]
async fn vscode_token_auth_still_accepts_a_valid_token() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect_with_origin(&handle, Some(JUPYTERLAB_ORIGIN)).await;

    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();

    let envelope = receive_envelope(&mut socket).await;
    assert_eq!(envelope.kind, MessageType::HelloAck);
    let payload = serde_json::from_value::<crate::HelloAckPayload>(envelope.payload).unwrap();
    assert_eq!(payload.adapter, "vscode");

    socket.close(None).await.ok();
    handle.shutdown().await;
}

#[tokio::test]
async fn a_failed_handshake_keeps_an_authenticated_adapter_connected() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;

    // Keep an unauthenticated connection waiting for hello, then authenticate
    // the real adapter. This is the state the audit describes: a second
    // connection is already in its handshake while the real adapter is active.
    // Send the wrong hello before the intruder's 5s authentication timeout.
    let (mut intruder, _) = connect(&handle).await;

    let (mut adapter, discovery) = connect(&handle).await;
    send_hello(&mut adapter, discovery.token).await;
    let mut state = handle.state.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(state, ConnectionState::Connected { .. })
        }),
    )
    .await
    .unwrap();

    send_hello(&mut intruder, "wrong-token".to_string()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(matches!(*state.borrow(), ConnectionState::Connected { .. }));

    intruder.close(None).await.ok();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(matches!(*state.borrow(), ConnectionState::Connected { .. }));

    adapter.close(None).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_state(&mut state, |state| {
            matches!(state, ConnectionState::WaitingForAdapter)
        }),
    )
    .await
    .unwrap();

    handle.shutdown().await;
}

#[tokio::test]
async fn rejects_wrong_protocol_version() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    let mut hello = Envelope::hello(discovery.token);
    hello.v = PROTOCOL_VERSION + 1;

    socket
        .send(Message::Text(hello.to_json().unwrap().into()))
        .await
        .unwrap();

    let reply = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = reply else {
        panic!("expected ack text");
    };
    let envelope = Envelope::parse(text.as_str()).unwrap();
    let payload = serde_json::from_value::<AckPayload>(envelope.payload).unwrap();

    assert_eq!(payload.status, AckStatus::Error);
    assert_eq!(payload.code.as_deref(), Some("version_mismatch"));

    handle.shutdown().await;
}

#[tokio::test]
async fn reconnects_after_disconnect() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let mut state = handle.state.clone();

    for attempt in 0..2 {
        let (mut socket, discovery) = connect(&handle).await;
        socket
            .send(Message::Text(
                Envelope::hello(discovery.token).to_json().unwrap().into(),
            ))
            .await
            .unwrap();

        let reply = socket.next().await.unwrap().unwrap();
        let Message::Text(text) = reply else {
            panic!("expected hello_ack text");
        };
        assert_eq!(
            Envelope::parse(text.as_str()).unwrap().kind,
            MessageType::HelloAck,
        );

        tokio::time::timeout(
            Duration::from_secs(2),
            wait_for_state(&mut state, |state| {
                matches!(state, ConnectionState::Connected { .. })
            }),
        )
        .await
        .unwrap();

        socket.close(None).await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            wait_for_state(&mut state, |state| {
                matches!(state, ConnectionState::WaitingForAdapter)
            }),
        )
        .await
        .unwrap();

        assert!(attempt < 1 || matches!(*state.borrow(), ConnectionState::WaitingForAdapter));
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn rejects_oversized_messages() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, _) = connect(&handle).await;

    let oversized = "x".repeat(MAX_WS_MESSAGE_SIZE + 1);
    let _ = socket.send(Message::Text(oversized.into())).await;

    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await {
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return true,
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(closed, "oversized frames must close the adapter socket");
    handle.shutdown().await;
}

#[tokio::test]
async fn a_silent_connection_does_not_block_the_real_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let silent = tokio::net::TcpStream::connect(("127.0.0.1", handle.port))
        .await
        .unwrap();

    let Ok(connected) = tokio::time::timeout(Duration::from_secs(2), connect(&handle)).await else {
        panic!("a silent TCP connection must not block the WebSocket handshake");
    };
    let (mut socket, discovery) = connected;
    tokio::time::timeout(
        Duration::from_secs(2),
        send_hello(&mut socket, discovery.token),
    )
    .await
    .expect("the real adapter must not wait for the silent connection");

    drop(silent);
    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn a_connection_that_stops_answering_pings_is_closed() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    for _ in 0..10 {
        if matches!(*handle.state.borrow(), ConnectionState::Connected { .. }) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(
        *handle.state.borrow(),
        ConnectionState::Connected { .. }
    ));

    for _ in 0..5 {
        tokio::time::advance(HEARTBEAT_INTERVAL).await;
        tokio::task::yield_now().await;
    }

    assert!(
        matches!(*handle.state.borrow(), ConnectionState::WaitingForAdapter),
        "a half-open adapter must be disconnected"
    );
    socket.close(None).await.ok();
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn a_connection_that_answers_pings_survives_the_heartbeat_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    for _ in 0..4 {
        tokio::time::advance(HEARTBEAT_INTERVAL).await;
        // Reading the Ping makes tungstenite queue the reply Pong; the next
        // read flushes it. `last_seen` must treat that Pong as liveness, or an
        // idle-but-healthy adapter dies at 60s.
        let message = socket.next().await;
        assert!(
            matches!(message, Some(Ok(Message::Ping(_)))),
            "the server must ping a live connection: {message:?}"
        );
    }

    assert!(
        matches!(*handle.state.borrow(), ConnectionState::Connected { .. }),
        "a connection that answers pings must outlive the heartbeat timeout"
    );
    socket.close(None).await.ok();
    handle.shutdown().await;
}

#[tokio::test]
async fn dropping_a_server_handle_removes_the_discovery_file() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start_test_server(directory.path(), vec![]).await;
    let runtime_path = handle.runtime_path.clone();
    assert!(runtime_path.exists());

    drop(handle);

    assert!(!runtime_path.exists());
}

#[test]
fn the_largest_terminal_ack_fits_the_websocket_limit() {
    // Worst case for JSON escaping: every output byte becomes `\u0001`.
    let output = "\u{1}".repeat(crate::course_session::MAX_COMMAND_OUTPUT_BYTES);
    let envelope = Envelope::new(
        MessageType::Ack,
        serde_json::to_value(AckPayload {
            status: AckStatus::Done,
            code: None,
            message: None,
            exit_code: Some(0),
            output: Some(output),
            output_truncated: Some(true),
            completion_marker_found: None,
            timed_out: None,
        })
        .unwrap(),
    );
    let json = envelope.to_json().unwrap();
    assert!(
        json.len() < MAX_WS_MESSAGE_SIZE,
        "largest terminal ACK is {} bytes, limit is {MAX_WS_MESSAGE_SIZE}",
        json.len()
    );
}

#[test]
fn recognizes_legacy_and_structured_terminal_timeouts() {
    use crate::server::connection::ack_timed_out;

    let mut payload = AckPayload {
        status: AckStatus::Cancelled,
        code: Some("terminal_timeout".to_string()),
        message: None,
        exit_code: None,
        output: None,
        output_truncated: None,
        completion_marker_found: None,
        timed_out: None,
    };
    assert!(ack_timed_out(&payload), "legacy code must stay recognized");

    payload.timed_out = Some(true);
    payload.code = None;
    assert!(ack_timed_out(&payload));

    payload.timed_out = None;
    payload.code = Some("user_stop".to_string());
    assert!(!ack_timed_out(&payload));
}
