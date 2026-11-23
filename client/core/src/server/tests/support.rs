use super::super::*;
use crate::course::{
    Action, ActionNarration, InsertTextAction, InsertTextTarget, PlaybackAction, PlaybackStep,
};
use crate::discovery::DiscoveryFile;
use std::path::Path;
use tokio_tungstenite::{MaybeTlsStream, connect_async};

pub(super) type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(super) async fn start_test_server(directory: &Path, actions: Vec<Action>) -> ServerHandle {
    let workspace = directory.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    start_server_with_path(
        workspace,
        directory.join("runtime.json"),
        vec![playback_step("main", "Main Step", actions)],
    )
    .await
    .unwrap()
}

pub(super) async fn start_test_server_for_adapter(
    directory: &Path,
    adapter: crate::AdapterId,
) -> ServerHandle {
    start_test_server_for_adapter_on_port(directory, adapter, 0).await
}

pub(super) async fn start_test_server_for_adapter_on_port(
    directory: &Path,
    adapter: crate::AdapterId,
    port: u16,
) -> ServerHandle {
    let workspace = directory.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    start_server_with_path_and_resume_for_adapter_on_port(
        workspace,
        directory.join("runtime.json"),
        vec![playback_step("main", "Main Step", Vec::new())],
        None,
        AdapterTarget::External(adapter),
        port,
    )
    .await
    .unwrap()
}

pub(super) async fn start_test_server_for_jupyterlab(
    directory: &Path,
    origin: &str,
) -> ServerHandle {
    let workspace = directory.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    start_server_with_path_and_resume_for_adapter_on_port_with_auth(
        workspace,
        directory.join("runtime.json"),
        vec![playback_step("main", "Main Step", Vec::new())],
        None,
        AdapterTarget::External(crate::AdapterId::Jupyterlab),
        AdapterAuth::JupyterLab {
            origin: origin.to_string(),
        },
        0,
    )
    .await
    .unwrap()
}

pub(super) fn insert_text(code: &str) -> Action {
    Action::InsertText(InsertTextAction {
        path: "src/main.rs".to_string(),
        target: InsertTextTarget {
            line: Some(1),
            at: None,
            placement: None,
            eof: false,
        },
        text: code.to_string(),
        text_path: None,
        speed_ms: Some(30),
        target_hold_ms: None,
    })
}

pub(super) fn playback_step(step_id: &str, step_title: &str, actions: Vec<Action>) -> PlaybackStep {
    playback_step_in_unit("main_unit", "Main Unit", step_id, step_title, actions)
}

pub(super) fn playback_step_in_unit(
    unit_path: &str,
    unit_title: &str,
    step_id: &str,
    step_title: &str,
    actions: Vec<Action>,
) -> PlaybackStep {
    PlaybackStep {
        parent_id: "ch_multi_step".to_string(),
        parent_title: "Step Grouping".to_string(),
        unit_title: unit_title.to_string(),
        unit_path: unit_path.to_string(),
        step_id: step_id.to_string(),
        step_title: step_title.to_string(),
        requires_human: false,
        challenge: false,
        actions: actions
            .into_iter()
            .map(|action| PlaybackAction {
                action,
                step_id: step_id.to_string(),
                narration: ActionNarration::default(),
            })
            .collect(),
    }
}

pub(super) fn ready_at(
    action_index: usize,
    action_total: usize,
    overall_action_total: usize,
) -> impl Fn(&PlaybackState) -> bool {
    move |state| {
        matches!(
            state,
            PlaybackState::Ready {
                position: Some(PlaybackPosition {
                    action_index: actual_action_index,
                    action_total: actual_action_total,
                    overall_action_index: 0,
                    overall_action_total: actual_overall_total,
                    ..
                })
            } if *actual_action_index == action_index
                && *actual_action_total == action_total
                && *actual_overall_total == overall_action_total
        )
    }
}

pub(super) fn playing_at(
    action_index: usize,
    action_total: usize,
) -> impl Fn(&PlaybackState) -> bool {
    move |state| {
        matches!(
            state,
            PlaybackState::Playing {
                position: PlaybackPosition {
                    action_index: actual_action_index,
                    action_total: actual_action_total,
                    ..
                },
                ..
            } if *actual_action_index == action_index
                && *actual_action_total == action_total
        )
    }
}

pub(super) fn paused_at(
    action_index: usize,
    action_total: usize,
) -> impl Fn(&PlaybackState) -> bool {
    move |state| {
        matches!(
            state,
            PlaybackState::Paused {
                position: PlaybackPosition {
                    action_index: actual_action_index,
                    action_total: actual_action_total,
                    ..
                },
                ..
            } if *actual_action_index == action_index
                && *actual_action_total == action_total
        )
    }
}

pub(super) fn unit_complete_at(
    action_index: usize,
    action_total: usize,
) -> impl Fn(&PlaybackState) -> bool {
    move |state| {
        matches!(
            state,
            PlaybackState::UnitComplete {
                position: PlaybackPosition {
                    action_index: actual_index,
                    action_total: actual_total,
                    overall_action_index: actual_overall_index,
                    overall_action_total: actual_overall_total,
                    ..
                }
            } if *actual_index == action_index
                && *actual_total == action_total
                && *actual_overall_index == action_total
                && *actual_overall_total == action_total
        )
    }
}

pub(super) async fn connect(handle: &ServerHandle) -> (ClientSocket, DiscoveryFile) {
    connect_with_origin(handle, None).await
}

pub(super) async fn connect_with_origin(
    handle: &ServerHandle,
    origin: Option<&str>,
) -> (ClientSocket, DiscoveryFile) {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let discovery: DiscoveryFile =
        serde_json::from_str(&std::fs::read_to_string(&handle.runtime_path).unwrap()).unwrap();
    let mut request = format!("ws://127.0.0.1:{}/", discovery.port)
        .into_client_request()
        .unwrap();
    if let Some(origin) = origin {
        request.headers_mut().insert(
            "origin",
            tokio_tungstenite::tungstenite::http::HeaderValue::from_str(origin).unwrap(),
        );
    }
    let (socket, _) =
        tokio::time::timeout(std::time::Duration::from_secs(2), connect_async(request))
            .await
            .expect("adapter connection timed out")
            .unwrap();

    (socket, discovery)
}

pub(super) async fn wait_for_state<F>(state: &mut watch::Receiver<ConnectionState>, predicate: F)
where
    F: Fn(&ConnectionState) -> bool,
{
    loop {
        if predicate(&state.borrow()) {
            return;
        }
        state.changed().await.unwrap();
    }
}

pub(super) async fn wait_for_playback<F>(state: &mut watch::Receiver<PlaybackState>, predicate: F)
where
    F: Fn(&PlaybackState) -> bool,
{
    loop {
        if predicate(&state.borrow()) {
            return;
        }
        state.changed().await.unwrap();
    }
}

pub(super) async fn send_hello(socket: &mut ClientSocket, token: String) {
    socket
        .send(Message::Text(
            Envelope::hello(token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
}

pub(super) async fn receive_envelope(socket: &mut ClientSocket) -> Envelope {
    loop {
        match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => return Envelope::parse(text.as_str()).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => panic!("expected text envelope"),
        }
    }
}

pub(super) async fn receive_action(socket: &mut ClientSocket) -> Envelope {
    let envelope = receive_envelope(socket).await;
    assert_eq!(envelope.kind, MessageType::Action);
    envelope
}

pub(super) async fn send_ack(socket: &mut ClientSocket, action_id: &str, status: &str) {
    send_ack_payload(socket, action_id, serde_json::json!({ "status": status })).await;
}

pub(super) async fn send_ack_payload(
    socket: &mut ClientSocket,
    action_id: &str,
    payload: serde_json::Value,
) {
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: action_id.to_string(),
                kind: MessageType::Ack,
                unit_path: None,
                payload,
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
}

pub(super) async fn send_terminal_ack(
    socket: &mut ClientSocket,
    action_id: String,
    exit_code: i32,
    output: &str,
) {
    socket
        .send(Message::Text(
            Envelope {
                v: PROTOCOL_VERSION,
                id: action_id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "done",
                    "exit_code": exit_code,
                    "output": output,
                    "output_truncated": false
                }),
            }
            .to_json()
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
}
