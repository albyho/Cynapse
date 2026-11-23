use super::*;

use super::connection::{authenticate, handle_connection};
use super::local_adapter::run_local_adapter;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_tungstenite::accept_hdr_async_with_config;
use tokio_tungstenite::tungstenite::handshake::server::Request;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

/// One MiB is far above any Action/Ack envelope; terminal output is truncated
/// before it reaches this size.
pub const MAX_WS_MESSAGE_SIZE: usize = 1 << 20;

const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// The `start_server*` entries are one implementation with its defaults peeled
/// off one at a time, because Rust has no default arguments: tests want only a
/// workspace and a step list, the model wants an Adapter target, and JupyterLab
/// wants a fixed port with Origin auth. `..._on_port_with_auth` below is that
/// implementation; every other name removes one argument from it.
pub async fn start_server_with_path(
    workspace_root: PathBuf,
    runtime_path: PathBuf,
    steps: Vec<PlaybackStep>,
) -> Result<ServerHandle, ServerError> {
    start_server_with_path_and_resume(workspace_root, runtime_path, steps, None).await
}

pub async fn start_server_with_path_and_resume(
    workspace_root: PathBuf,
    runtime_path: PathBuf,
    steps: Vec<PlaybackStep>,
    resume: Option<PlaybackResume>,
) -> Result<ServerHandle, ServerError> {
    start_server_with_path_and_resume_for_adapter(
        workspace_root,
        runtime_path,
        steps,
        resume,
        AdapterTarget::External(AdapterId::Vscode),
    )
    .await
}

/// Starts the Protocol server for the Adapter the Course asks for, or for the
/// built-in local adapter when it asks for none.
pub async fn start_server_with_path_and_resume_for_adapter(
    workspace_root: PathBuf,
    runtime_path: PathBuf,
    steps: Vec<PlaybackStep>,
    resume: Option<PlaybackResume>,
    target: AdapterTarget,
) -> Result<ServerHandle, ServerError> {
    start_server_with_path_and_resume_for_adapter_on_port(
        workspace_root,
        runtime_path,
        steps,
        resume,
        target,
        0,
    )
    .await
}

/// Binds the Protocol server to a fixed loopback control port. JupyterLab's
/// extension uses this port; `0` still asks the OS for an ephemeral port.
pub async fn start_server_with_path_and_resume_for_adapter_on_port(
    workspace_root: PathBuf,
    runtime_path: PathBuf,
    steps: Vec<PlaybackStep>,
    resume: Option<PlaybackResume>,
    target: AdapterTarget,
    port: u16,
) -> Result<ServerHandle, ServerError> {
    start_server_with_path_and_resume_for_adapter_on_port_with_auth(
        workspace_root,
        runtime_path,
        steps,
        resume,
        target,
        AdapterAuth::Token,
        port,
    )
    .await
}

/// The implementation every entry above forwards to. `target` decides the
/// `hello_ack` connection name and whether the local adapter loop runs; `auth`
/// decides how a connection proves it belongs here — VS Code keeps the token,
/// JupyterLab matches Origin.
#[allow(clippy::result_large_err)]
pub async fn start_server_with_path_and_resume_for_adapter_on_port_with_auth(
    workspace_root: PathBuf,
    runtime_path: PathBuf,
    steps: Vec<PlaybackStep>,
    resume: Option<PlaybackResume>,
    target: AdapterTarget,
    auth: AdapterAuth,
    port: u16,
) -> Result<ServerHandle, ServerError> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| {
            if port != 0 && error.kind() == std::io::ErrorKind::AddrInUse {
                ServerError::PortInUse { port }
            } else {
                ServerError::Io(error)
            }
        })?;
    let bound_port = listener.local_addr()?.port();
    let discovery = new_discovery(bound_port, workspace_root.to_string_lossy().to_string());
    let discovery_handle = DiscoveryHandle::write(runtime_path, &discovery)?;
    let runtime_path = discovery_handle.path().to_path_buf();
    let (state_tx, state_rx) = watch::channel(ConnectionState::WaitingForAdapter);
    let (adapter_name, use_local_adapter) = match target {
        AdapterTarget::External(adapter) => (adapter.connection_name(), false),
        AdapterTarget::Local => ("local", true),
    };
    let plan = ServerPlaybackPlan::new(steps, adapter_name);
    let cursor = resume
        .as_ref()
        .and_then(|resume| resume_cursor(&plan.steps, resume))
        .or_else(|| first_cursor(&plan.steps));
    let initial_playback = initial_playback_state(&plan, resume.as_ref());
    let (playback_tx, playback_rx) = watch::channel(initial_playback);
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    let commands = PlaybackCommandSender {
        commands: command_tx,
    };

    let token = discovery.token.clone();
    let handle_token = discovery.token.clone();
    let local_adapter_task =
        use_local_adapter.then(|| tokio::spawn(run_local_adapter(bound_port, token.clone())));
    let task_discovery = discovery_handle.clone();
    let task = tokio::spawn(async move {
        let mut plan = plan;
        let mut playback_session = PlaybackSession {
            cursor,
            state: playback_tx.borrow().clone(),
            pending_boundary: None,
            correlation_id: None,
            skip_waits: false,
            skip_waits_through: resume_skip_waits_through(&plan, resume.as_ref()),
            force_skip_waits: false,
            skip_unit_final_wait: false,
        };
        let adapter_name = plan.adapter_name;
        let (connection_tx, mut connection_rx) = mpsc::unbounded_channel();
        // A newer authenticated tab replaces the active one: the newest
        // generation wins, and the older connection is closed with an
        // "inactive" reason. Accepting stays responsive while a connection is
        // handled, so the replacement handshake never waits in the backlog.
        let newest_generation = std::sync::Arc::new(AtomicU64::new(0));
        let (generation_tx, generation_rx) = watch::channel(0u64);

        let accept_loop = async {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        let (stream, _) = match accepted {
                            Ok(accepted) => accepted,
                            Err(error) => {
                                log::warn!(
                                    target: "cynapse.server",
                                    "accept.failed error={error}; retrying"
                                );
                                tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                                continue;
                            }
                        };
                        let token = token.clone();
                        let auth = auth.clone();
                        let state_tx = state_tx.clone();
                        let connection_tx = connection_tx.clone();
                        let newest_generation = newest_generation.clone();
                        let generation_tx = generation_tx.clone();
                        tokio::spawn(async move {
                            let mut origin = None;
                            let Ok(socket) = accept_hdr_async_with_config(
                                stream,
                                |request: &Request, response| {
                                    origin = request
                                        .headers()
                                        .get("origin")
                                        .and_then(|value| value.to_str().ok())
                                        .map(str::to_string);
                                    Ok(response)
                                },
                                Some(
                                    WebSocketConfig::default()
                                        .max_message_size(Some(MAX_WS_MESSAGE_SIZE))
                                        .max_frame_size(Some(MAX_WS_MESSAGE_SIZE)),
                                ),
                            )
                            .await else { return; };
                            if let Some(socket) = authenticate(
                                socket,
                                &auth,
                                origin.as_deref(),
                                &token,
                                adapter_name,
                                &state_tx,
                            )
                            .await
                            {
                                let generation =
                                    newest_generation.fetch_add(1, Ordering::SeqCst) + 1;
                                let _ = generation_tx.send(generation);
                                let _ = connection_tx.send((socket, generation));
                            }
                        });
                    }
                }
            }
        };

        let connection_loop = async {
            while let Some((socket, generation)) = connection_rx.recv().await {
                handle_connection(
                    socket,
                    AdapterGeneration {
                        generation,
                        receiver: generation_rx.clone(),
                        newest: newest_generation.clone(),
                    },
                    &state_tx,
                    &playback_tx,
                    &mut plan,
                    &mut playback_session,
                    &mut command_rx,
                )
                .await;
            }
        };

        tokio::select! {
            _ = accept_loop => {}
            _ = connection_loop => {}
        }

        let _ = task_discovery.remove();
    });

    Ok(ServerHandle {
        port: bound_port,
        token: handle_token,
        runtime_path,
        state: state_rx,
        playback: playback_rx,
        commands,
        adapter_name,
        discovery: discovery_handle,
        local_adapter_task,
        shutdown: Some(shutdown_tx),
        task,
    })
}
