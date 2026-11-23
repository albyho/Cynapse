//! Shared fakes, course writers, and wait helpers for SessionService tests.
//!
//! Fake Adapters:
//! - `fake_adapter`: answers actions, checks, and resets with `done`; stays connected.
//! - `fake_adapter_fails_environment`: fails terminal environment checks with `environment_failed`.
//! - `fake_adapter_closes_after_environment`: ACKs an environment check with `exit_code` and
//!   `output`, then the terminal cleanup reset, then closes; use it to fail the next `ReplacePlan`.
//! - `fake_adapter_holds_terminal_command`: holds a terminal command until the test releases it.
//! - `fake_adapter_holds_workspace_reset`: holds a workspace reset until the test releases it.
//! - `fake_adapter_rejects_workspace_reset`: returns an error for a workspace reset.
//!
//! Environment checks run `TerminalEnsure`, then `TerminalCommand` (the ACK
//! needs `exit_code` and `output`), then a terminal-cleanup `WorkspaceReset`.
//! See `client/core/src/server/environment_check.rs` for the exact ACK fields.

use super::*;
use cynapse_client_core::{
    ChallengeStatus, DiscoveryFile, Envelope, HelloAckPayload, JupyterLabProvisioner,
    JupyterLabRuntime, MessageType, PROTOCOL_VERSION,
};
use futures_util::{SinkExt, StreamExt};
use std::fs;
use std::future::Future;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// Drives a service future without an ambient Tokio reactor. Use this for any
/// reset/cleanup path that the app schedules with `cx.background_spawn`.
pub(super) fn drive_without_runtime<F: Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

pub(super) fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[derive(Default)]
pub(super) struct FakeJupyterLabState {
    pub(super) starts: std::sync::Mutex<Vec<PathBuf>>,
    pub(super) stops: std::sync::atomic::AtomicUsize,
    pub(super) cleanups: std::sync::atomic::AtomicUsize,
    pub(super) running: std::sync::atomic::AtomicBool,
}

pub(super) struct FakeJupyterLabProvisioner {
    pub(super) state: Arc<FakeJupyterLabState>,
}

impl JupyterLabProvisioner for FakeJupyterLabProvisioner {
    fn start(
        &self,
        course: &cynapse_client_core::Course,
    ) -> Result<Box<dyn JupyterLabRuntime>, String> {
        let workspace = course.workspace_path();
        self.state.starts.lock().unwrap().push(workspace.clone());
        self.state
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Box::new(FakeJupyterLabRuntime {
            state: self.state.clone(),
            workspace,
            url: "http://127.0.0.1:18890/lab?token=launch".to_string(),
        }))
    }

    fn cleanup_stale(&self) {
        self.state
            .cleanups
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(super) struct FakeJupyterLabRuntime {
    state: Arc<FakeJupyterLabState>,
    workspace: PathBuf,
    url: String,
}

impl JupyterLabRuntime for FakeJupyterLabRuntime {
    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn browser_url(&self) -> String {
        self.url.clone()
    }

    fn server_port(&self) -> u16 {
        18890
    }

    fn is_running(&mut self) -> bool {
        self.state.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn stop(&mut self) {
        self.state
            .running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.state
            .stops
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(super) fn jupyterlab_service(
    directory: &Path,
    provisioner: Arc<FakeJupyterLabProvisioner>,
    control_port: u16,
    handshake_timeout: Duration,
) -> (SessionService, UnboundedReceiver<ClientEvent>) {
    let (events, receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at_with_jupyterlab(
        events,
        directory.join("state.db"),
        provisioner,
        control_port,
        handshake_timeout,
    )
    .unwrap();
    let package = write_adapter_course(directory, "jupyterlab");
    futures::executor::block_on(service.open_course(package)).unwrap();
    (service, receiver)
}

pub(super) fn write_final_challenge_course(root: &Path) -> PathBuf {
    let package = write_challenge_course(root);
    let manifest = serde_json::json!({
        "id": "challenge_demo",
        "title": "Challenge Demo",
        "version": "1.0.0",
        "adapters": ["vscode"],
        "workspace": "workspace",
        "children": [{
            "id": "ch",
            "title": "Challenge",
            "children": [{
                "id": "main",
                "title": "Unit",
                "steps": [{ "step_file": "steps/check.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    package
}

pub(super) fn write_challenge_course(root: &Path) -> PathBuf {
    write_challenge_course_with_timeout(root, 10_000)
}

pub(super) fn write_start_environment_course(root: &Path) -> PathBuf {
    let package = write_environment_step_course(root);
    let manifest = serde_json::json!({
        "id": "environment_start_demo",
        "title": "Environment Start Demo",
        "version": "1.0.0",
        "adapters": ["vscode"],
        "workspace": "workspace",
        "environment": [{
            "id": "rustc",
            "command": "rustc --version",
            "minimum_version": "1.85.0"
        }],
        "children": [{
            "id": "ch",
            "title": "Chapter",
            "children": [{
                "id": "main",
                "title": "Unit",
                "steps": [{ "step_file": "steps/present.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(package.join("seeds/ch/main")).unwrap();
    fs::write(package.join("seeds/ch/main/seed.txt"), "seed").unwrap();
    package
}

pub(super) fn write_advance_environment_course(root: &Path) -> PathBuf {
    let package = write_environment_step_course(root);
    let manifest = serde_json::json!({
        "id": "environment_advance_demo",
        "title": "Environment Advance Demo",
        "version": "1.0.0",
        "adapters": ["vscode"],
        "workspace": "workspace",
        "children": [{
            "id": "ch",
            "title": "Chapter",
            "children": [{
                "id": "first",
                "title": "First",
                "steps": [{ "step_file": "steps/present.json" }]
            }, {
                "id": "second",
                "title": "Second",
                "environment": [{
                    "id": "rustc",
                    "command": "rustc --version",
                    "minimum_version": "1.85.0"
                }],
                "steps": [{ "step_file": "steps/present.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(package.join("seeds/ch/second")).unwrap();
    fs::write(package.join("seeds/ch/second/seed.txt"), "seed").unwrap();
    package
}

pub(super) fn write_environment_step_course(root: &Path) -> PathBuf {
    let package = root.join("course");
    fs::create_dir_all(package.join("workspace")).unwrap();
    fs::create_dir_all(package.join("steps")).unwrap();
    let step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{ "type": "Present" }]
    });
    fs::write(
        package.join("steps/present.json"),
        serde_json::to_string_pretty(&step).unwrap(),
    )
    .unwrap();
    package
}

pub(super) fn write_mixed_adapter_course(root: &Path) -> PathBuf {
    let package = root.join("course");
    fs::create_dir_all(package.join("workspace")).unwrap();
    fs::create_dir_all(package.join("steps")).unwrap();
    let manifest = serde_json::json!({
        "id": "mixed_adapter_demo",
        "title": "Mixed Adapter Demo",
        "version": "1.0.0",
        "adapters": ["jupyterlab"],
        "workspace": "workspace",
        "children": [{
            "id": "ch",
            "title": "Chapter",
            "children": [{
                "id": "jupyter",
                "title": "Jupyter Unit",
                "steps": [{ "step_file": "steps/present.json" }]
            }, {
                "id": "vscode",
                "title": "VS Code Unit",
                "adapters": ["vscode"],
                "steps": [{ "step_file": "steps/present.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{ "type": "Present" }]
    });
    fs::write(
        package.join("steps/present.json"),
        serde_json::to_string_pretty(&step).unwrap(),
    )
    .unwrap();
    package
}

/// A Course that names no Adapter. Admissible only because every Action is
/// Client-local, and playable only through the built-in local adapter
/// (spec §4.2) — no external App connects to it.
pub(super) fn write_adapterless_course(root: &Path) -> PathBuf {
    let package = root.join("course");
    fs::create_dir_all(package.join("workspace")).unwrap();
    fs::create_dir_all(package.join("steps")).unwrap();
    let manifest = serde_json::json!({
        "id": "adapterless_demo",
        "title": "Adapterless Demo",
        "version": "1.0.0",
        "workspace": "workspace",
        "children": [{
            "id": "ch",
            "title": "Chapter",
            "children": [{
                "id": "main",
                "title": "Unit",
                "steps": [{ "step_file": "steps/present.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{ "type": "Present" }]
    });
    fs::write(
        package.join("steps/present.json"),
        serde_json::to_string_pretty(&step).unwrap(),
    )
    .unwrap();
    package
}

/// A Course for VS Code whose startup Unit is local-only: the Unit clears its
/// `adapters` and keeps every Action Client-legal, so it is admissible with a
/// warning (spec §4.2) and must play without the external App.
pub(super) fn write_local_only_unit_course(root: &Path) -> PathBuf {
    let package = root.join("course");
    fs::create_dir_all(package.join("workspace")).unwrap();
    fs::create_dir_all(package.join("steps")).unwrap();
    let manifest = serde_json::json!({
        "id": "local_only_unit_demo",
        "title": "Local-Only Unit Demo",
        "version": "1.0.0",
        "adapters": ["vscode"],
        "workspace": "workspace",
        "children": [{
            "id": "ch",
            "title": "Chapter",
            "children": [{
                "id": "solo",
                "title": "Local-Only Unit",
                "adapters": [],
                "steps": [{ "step_file": "steps/present.json" }]
            }, {
                "id": "external",
                "title": "External Unit",
                "steps": [{ "step_file": "steps/present.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{ "type": "Present" }]
    });
    fs::write(
        package.join("steps/present.json"),
        serde_json::to_string_pretty(&step).unwrap(),
    )
    .unwrap();
    package
}

pub(super) fn write_adapter_course(root: &Path, adapter: &str) -> PathBuf {
    let package = root.join("course");
    fs::create_dir_all(package.join("workspace")).unwrap();
    fs::create_dir_all(package.join("steps")).unwrap();
    let manifest = serde_json::json!({
        "id": "adapter_demo",
        "title": "Adapter Demo",
        "version": "1.0.0",
        "adapters": [adapter],
        "workspace": "workspace",
        "children": [{
            "id": "ch",
            "title": "Chapter",
            "children": [{
                "id": "main",
                "title": "Unit",
                "steps": [{ "step_file": "steps/present.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{ "type": "Present" }]
    });
    fs::write(
        package.join("steps/present.json"),
        serde_json::to_string_pretty(&step).unwrap(),
    )
    .unwrap();
    package
}

pub(super) fn write_challenge_course_with_timeout(root: &Path, check_timeout_ms: u64) -> PathBuf {
    let package = root.join("course");
    fs::create_dir_all(package.join("workspace")).unwrap();
    fs::create_dir_all(package.join("steps")).unwrap();
    let manifest = serde_json::json!({
        "id": "challenge_demo",
        "title": "Challenge Demo",
        "version": "1.0.0",
        "adapters": ["vscode"],
        "workspace": "workspace",
        "children": [{
            "id": "ch",
            "title": "Challenge",
            "children": [{
                "id": "main",
                "title": "Unit",
                "steps": [{ "step_file": "steps/check.json" }]
            }, {
                "id": "next",
                "title": "Next Unit",
                "steps": [{ "step_file": "steps/next.json" }]
            }]
        }]
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let step = serde_json::json!({
        "step_id": "check",
        "title": "Check",
        "actions": [
            { "type": "Present" },
            { "type": "TerminalEnsure", "session_key": "main", "cwd": "." },
            {
                "type": "TerminalCommand",
                "session_key": "main",
                "cwd": ".",
                "command": "node --test",
                "accepted_exit_codes": [0],
                "expected_output": [],
                "timeout_ms": 10000
            }
        ],
        "challenge": {
            "instructions": "Run the check.",
            "checks": [{
                "type": "COMMAND",
                "session_key": "main",
                "cwd": ".",
                "command": "true",
                "accepted_exit_codes": [0],
                "expected_output": [],
                "timeout_ms": check_timeout_ms
            }],
            "allow_skip": true
        }
    });
    fs::write(
        package.join("steps/check.json"),
        serde_json::to_string_pretty(&step).unwrap(),
    )
    .unwrap();
    let next_step = serde_json::json!({
        "step_id": "next",
        "title": "Next",
        "actions": [{ "type": "Present" }]
    });
    fs::write(
        package.join("steps/next.json"),
        serde_json::to_string_pretty(&next_step).unwrap(),
    )
    .unwrap();
    package
}

pub(super) async fn fake_adapter_disconnect_after_action(runtime_path: PathBuf) {
    let discovery = loop {
        if let Some(discovery) = fs::read_to_string(&runtime_path)
            .ok()
            .and_then(|text| serde_json::from_str::<DiscoveryFile>(&text).ok())
        {
            break discovery;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        match envelope.kind {
            MessageType::Action => return,
            MessageType::WorkspaceReset => {
                let ack = Envelope {
                    v: PROTOCOL_VERSION,
                    id: envelope.id,
                    kind: MessageType::Ack,
                    unit_path: None,
                    payload: serde_json::json!({ "status": "done" }),
                };
                socket
                    .send(Message::Text(ack.to_json().unwrap().into()))
                    .await
                    .unwrap();
            }
            _ => {}
        }
    }
}

pub(super) async fn wait_for_connection(
    receiver: &mut UnboundedReceiver<ClientEvent>,
    connected: bool,
) {
    loop {
        let event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for connection")
            .expect("event channel closed");
        if let ClientEvent::Connection(state) = event
            && matches!(state, ConnectionState::Connected { .. }) == connected
        {
            return;
        }
    }
}

pub(super) async fn wait_for_model_connection(
    model: &mut crate::ClientModel,
    receiver: &mut UnboundedReceiver<ClientEvent>,
    connected: bool,
) {
    loop {
        let event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for connection")
            .expect("event channel closed");
        let is_connection = matches!(&event, ClientEvent::Connection(_));
        let state = match &event {
            ClientEvent::Connection(state) => Some(state.clone()),
            _ => None,
        };
        model.apply(event);
        if is_connection
            && state.is_some_and(|state| {
                matches!(state, ConnectionState::Connected { .. }) == connected
            })
        {
            return;
        }
    }
}

pub(super) async fn wait_for_model(
    model: &mut crate::ClientModel,
    receiver: &mut UnboundedReceiver<ClientEvent>,
    predicate: impl Fn(&crate::ClientModel) -> bool,
) {
    loop {
        let event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for model state")
            .expect("event channel closed");
        model.apply(event);
        if predicate(model) {
            return;
        }
    }
}

pub(super) async fn fake_adapter_ignores_check(
    runtime_path: PathBuf,
    cancels: UnboundedSender<String>,
) {
    let discovery = loop {
        if let Some(discovery) = fs::read_to_string(&runtime_path)
            .ok()
            .and_then(|text| serde_json::from_str::<DiscoveryFile>(&text).ok())
        {
            break discovery;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    let mut pending_check: Option<String> = None;
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        match envelope.kind {
            MessageType::Action => {
                let ack = Envelope {
                    v: PROTOCOL_VERSION,
                    id: envelope.id,
                    kind: MessageType::Ack,
                    unit_path: None,
                    payload: serde_json::json!({ "status": "done" }),
                };
                socket
                    .send(Message::Text(ack.to_json().unwrap().into()))
                    .await
                    .unwrap();
            }
            MessageType::Check => {
                pending_check = Some(envelope.id);
            }
            MessageType::Cancel => {
                let target = envelope.payload["target_id"].as_str();
                if target.is_some() && target == pending_check.as_deref() {
                    let _ = cancels.send(target.unwrap_or_default().to_string());
                }
            }
            MessageType::WorkspaceReset => {
                let ack = Envelope {
                    v: PROTOCOL_VERSION,
                    id: envelope.id,
                    kind: MessageType::Ack,
                    unit_path: None,
                    payload: serde_json::json!({ "status": "done" }),
                };
                socket
                    .send(Message::Text(ack.to_json().unwrap().into()))
                    .await
                    .unwrap();
            }
            _ => {}
        }
    }
}

pub(super) async fn fake_adapter_fails_action(runtime_path: PathBuf) {
    let discovery = loop {
        if let Some(discovery) = fs::read_to_string(&runtime_path)
            .ok()
            .and_then(|text| serde_json::from_str::<DiscoveryFile>(&text).ok())
        {
            break discovery;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        let ack = match envelope.kind {
            MessageType::Action => Envelope {
                v: PROTOCOL_VERSION,
                id: envelope.id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({
                    "status": "error",
                    "code": "action_failed",
                    "message": "boom"
                }),
            },
            MessageType::WorkspaceReset => Envelope {
                v: PROTOCOL_VERSION,
                id: envelope.id,
                kind: MessageType::Ack,
                unit_path: None,
                payload: serde_json::json!({ "status": "done" }),
            },
            _ => continue,
        };
        socket
            .send(Message::Text(ack.to_json().unwrap().into()))
            .await
            .unwrap();
    }
}

pub(super) async fn fake_adapter_fails_environment(runtime_path: PathBuf) {
    let discovery = wait_for_discovery(&runtime_path).await;
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        let payload = match envelope.kind {
            MessageType::Action
                if matches!(
                    envelope
                        .payload
                        .get("type")
                        .and_then(|value| value.as_str()),
                    Some("TerminalEnsure" | "TerminalCommand")
                ) =>
            {
                serde_json::json!({
                    "status": "error",
                    "code": "environment_failed",
                    "message": "boom"
                })
            }
            MessageType::Action | MessageType::WorkspaceReset => {
                serde_json::json!({ "status": "done" })
            }
            _ => continue,
        };
        let ack = Envelope {
            v: PROTOCOL_VERSION,
            id: envelope.id,
            kind: MessageType::Ack,
            unit_path: None,
            payload,
        };
        socket
            .send(Message::Text(ack.to_json().unwrap().into()))
            .await
            .unwrap();
    }
}

/// ACKs the environment check for `unit_path` (including `exit_code` and
/// `output`), then the terminal cleanup reset, then closes the connection.
/// Use this to make the next `ReplacePlan` fail with no connection loop.
pub(super) async fn fake_adapter_closes_after_environment(
    runtime_path: PathBuf,
    unit_path: &'static str,
) {
    let discovery = wait_for_discovery(&runtime_path).await;
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    let mut environment_checked = false;
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        let target_unit = envelope.unit_path.as_deref().or_else(|| {
            envelope
                .payload
                .get("unit_path")
                .and_then(|value| value.as_str())
        });
        let action_type = envelope
            .payload
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("-");
        let is_target = target_unit == Some(unit_path)
            && match envelope.kind {
                MessageType::Action => action_type == "TerminalCommand",
                MessageType::Check => true,
                _ => false,
            };
        let payload = if is_target {
            serde_json::json!({
                "status": "done",
                "exit_code": 0,
                "output": "rustc 1.85.0 (test)"
            })
        } else {
            match envelope.kind {
                MessageType::Action | MessageType::Check | MessageType::WorkspaceReset => {
                    serde_json::json!({ "status": "done" })
                }
                _ => continue,
            }
        };
        let ack = Envelope {
            v: PROTOCOL_VERSION,
            id: envelope.id,
            kind: MessageType::Ack,
            unit_path: None,
            payload,
        };
        socket
            .send(Message::Text(ack.to_json().unwrap().into()))
            .await
            .unwrap();
        if is_target {
            environment_checked = true;
        } else if environment_checked && envelope.kind == MessageType::WorkspaceReset {
            let _ = socket.close(None).await;
            break;
        }
    }
}

pub(super) async fn fake_adapter_holds_terminal_command(runtime_path: PathBuf) {
    let discovery = loop {
        if let Some(discovery) = fs::read_to_string(&runtime_path)
            .ok()
            .and_then(|text| serde_json::from_str::<DiscoveryFile>(&text).ok())
        {
            break discovery;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        let terminal_command = envelope.kind == MessageType::Action
            && envelope
                .payload
                .get("type")
                .and_then(|value| value.as_str())
                == Some("TerminalCommand");
        if terminal_command {
            continue;
        }
        let payload = match envelope.kind {
            MessageType::Action => serde_json::json!({ "status": "done" }),
            MessageType::Check => serde_json::json!({
                "status": "done",
                "output": "ok",
                "output_truncated": false
            }),
            MessageType::WorkspaceReset => serde_json::json!({ "status": "done" }),
            _ => continue,
        };
        let ack = Envelope {
            v: PROTOCOL_VERSION,
            id: envelope.id,
            kind: MessageType::Ack,
            unit_path: None,
            payload,
        };
        socket
            .send(Message::Text(ack.to_json().unwrap().into()))
            .await
            .unwrap();
    }
}

pub(super) type TestSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Model tests use the fake JupyterLab origin so the fixed-port control server
/// exercises the Origin path; VS Code token auth ignores the extra header.
pub(super) async fn connect_test_adapter(
    port: u16,
) -> Result<TestSocket, tokio_tungstenite::tungstenite::Error> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = format!("ws://127.0.0.1:{port}/").into_client_request()?;
    request.headers_mut().insert(
        "origin",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static("http://127.0.0.1:18890"),
    );
    let (socket, _) = connect_async(request).await?;
    Ok(socket)
}

pub(super) async fn connect_fake_adapter(runtime_path: &Path) -> (TestSocket, DiscoveryFile) {
    loop {
        let discovery = wait_for_discovery(runtime_path).await;
        if let Ok(socket) = connect_test_adapter(discovery.port).await {
            return (socket, discovery);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(super) async fn fake_adapter(runtime_path: PathBuf) {
    let (mut socket, discovery) = connect_fake_adapter(&runtime_path).await;
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        let payload = match envelope.kind {
            MessageType::Action => serde_json::json!({ "status": "done" }),
            MessageType::Check => serde_json::json!({
                "status": "done",
                "output": "ok",
                "output_truncated": false
            }),
            MessageType::WorkspaceReset => serde_json::json!({ "status": "done" }),
            _ => continue,
        };
        let ack = Envelope {
            v: PROTOCOL_VERSION,
            id: envelope.id,
            kind: MessageType::Ack,
            unit_path: None,
            payload,
        };
        socket
            .send(Message::Text(ack.to_json().unwrap().into()))
            .await
            .unwrap();
    }
}

pub(super) async fn fake_adapter_holds_workspace_reset(
    runtime_path: PathBuf,
    seen: UnboundedSender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
) {
    let discovery = wait_for_discovery(&runtime_path).await;
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    let mut release = Some(release);
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        let payload = match envelope.kind {
            MessageType::Action => serde_json::json!({ "status": "done" }),
            MessageType::WorkspaceReset => {
                if envelope
                    .payload
                    .get("close_tabs")
                    .and_then(|value| value.as_bool())
                    == Some(false)
                {
                    serde_json::json!({ "status": "done" })
                } else {
                    let _ = seen.send(());
                    release
                        .take()
                        .expect("one workspace reset per test")
                        .await
                        .expect("release the held ACK");
                    serde_json::json!({ "status": "done" })
                }
            }
            _ => continue,
        };
        let ack = Envelope {
            v: PROTOCOL_VERSION,
            id: envelope.id,
            kind: MessageType::Ack,
            unit_path: None,
            payload,
        };
        socket
            .send(Message::Text(ack.to_json().unwrap().into()))
            .await
            .unwrap();
    }
}

pub(super) async fn fake_adapter_rejects_workspace_reset(runtime_path: PathBuf) {
    let discovery = wait_for_discovery(&runtime_path).await;
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(envelope) = Envelope::parse(text.as_str()) else {
            continue;
        };
        let payload = match envelope.kind {
            MessageType::Action => serde_json::json!({ "status": "done" }),
            MessageType::WorkspaceReset => {
                if envelope
                    .payload
                    .get("close_tabs")
                    .and_then(|value| value.as_bool())
                    == Some(false)
                {
                    serde_json::json!({ "status": "done" })
                } else {
                    serde_json::json!({
                        "status": "error",
                        "code": "notebook_close_incomplete",
                        "message": "Notebook still open"
                    })
                }
            }
            _ => continue,
        };
        let ack = Envelope {
            v: PROTOCOL_VERSION,
            id: envelope.id,
            kind: MessageType::Ack,
            unit_path: None,
            payload,
        };
        socket
            .send(Message::Text(ack.to_json().unwrap().into()))
            .await
            .unwrap();
    }
}

pub(super) async fn fake_adapter_reconnecting(runtime_path: PathBuf) {
    loop {
        let discovery = fs::read_to_string(&runtime_path)
            .ok()
            .and_then(|text| serde_json::from_str::<DiscoveryFile>(&text).ok());
        if let Some(discovery) = discovery
            && let Ok(mut socket) = connect_test_adapter(discovery.port).await
        {
            socket
                .send(Message::Text(
                    Envelope::hello(discovery.token).to_json().unwrap().into(),
                ))
                .await
                .unwrap();
            while let Some(Ok(message)) = socket.next().await {
                let Message::Text(text) = message else {
                    continue;
                };
                let Ok(envelope) = Envelope::parse(text.as_str()) else {
                    continue;
                };
                let payload = match envelope.kind {
                    MessageType::Action => serde_json::json!({ "status": "done" }),
                    MessageType::Check => serde_json::json!({
                        "status": "done",
                        "output": "ok",
                        "output_truncated": false
                    }),
                    MessageType::WorkspaceReset => serde_json::json!({ "status": "done" }),
                    _ => continue,
                };
                let ack = Envelope {
                    v: PROTOCOL_VERSION,
                    id: envelope.id,
                    kind: MessageType::Ack,
                    unit_path: None,
                    payload,
                };
                socket
                    .send(Message::Text(ack.to_json().unwrap().into()))
                    .await
                    .unwrap();
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(super) async fn fake_adapter_hello_ack(runtime_path: PathBuf) -> String {
    let discovery = wait_for_discovery(&runtime_path).await;
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json().unwrap().into(),
        ))
        .await
        .unwrap();
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                let Ok(envelope) = Envelope::parse(text.as_str()) else {
                    continue;
                };
                if envelope.kind == MessageType::HelloAck {
                    return serde_json::from_value::<HelloAckPayload>(envelope.payload)
                        .unwrap()
                        .adapter;
                }
            }
            Some(Ok(_)) => continue,
            _ => panic!("adapter connection closed before hello_ack"),
        }
    }
}

pub(super) async fn wait_for_discovery(runtime_path: &Path) -> DiscoveryFile {
    loop {
        if let Some(discovery) = fs::read_to_string(runtime_path)
            .ok()
            .and_then(|text| serde_json::from_str::<DiscoveryFile>(&text).ok())
        {
            return discovery;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(super) async fn wait_for_playback(
    receiver: &mut UnboundedReceiver<ClientEvent>,
    predicate: impl Fn(&PlaybackState) -> bool,
) -> PlaybackState {
    loop {
        let event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for playback")
            .expect("event channel closed");
        if let ClientEvent::Playback(playback) = event
            && predicate(&playback)
        {
            return *playback;
        }
    }
}

pub(super) async fn wait_for_challenge_status(
    receiver: &mut UnboundedReceiver<ClientEvent>,
    unit_path: &str,
    step_id: &str,
    expected: ChallengeStatus,
) {
    loop {
        let event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for course state")
            .expect("event channel closed");
        let ClientEvent::Course(state) = event else {
            continue;
        };
        let Some(course) = state.active_course.as_ref() else {
            continue;
        };
        let Some(unit) = find_unit(&course.tree, unit_path) else {
            continue;
        };
        let Some(step) = unit.steps.iter().find(|step| step.step_id == step_id) else {
            continue;
        };
        if step.challenge_status == expected {
            return;
        }
    }
}

pub(super) fn find_unit<'a>(
    nodes: &'a [cynapse_client_core::CourseTreeNodeView],
    path: &str,
) -> Option<&'a cynapse_client_core::UnitView> {
    for node in nodes {
        if let Some(unit) = node.unit.as_ref().filter(|unit| unit.path == path) {
            return Some(unit);
        }
        if let Some(unit) = find_unit(&node.children, path) {
            return Some(unit);
        }
    }
    None
}
