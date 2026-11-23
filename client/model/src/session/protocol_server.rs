//! Active Protocol Server slot and its installation seam.
//!
//! The slot owns the `ServerHandle` and its watch task. Callers describe whether
//! the installation is a controlled replacement or an external connection; the
//! slot hides the watch-channel forwarding and the initial-event policy.

use super::*;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplacementKind {
    /// A restart caused by the Client (Start, Reset, Adapter switch). Its
    /// transient Waiting state must not look like an Adapter loss.
    Controlled,
    /// A server installed for the initial or external lifecycle. Its initial
    /// state is published as-is.
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerPlan {
    KeepListening,
    Reuse,
    StartFor(AdapterId),
}

/// Decide whether the current server can serve the next Unit.
pub(super) fn plan_server(current: Option<AdapterId>, target: Option<AdapterId>) -> ServerPlan {
    match target {
        None => ServerPlan::KeepListening,
        Some(target) if current == Some(target) => ServerPlan::Reuse,
        Some(target) => ServerPlan::StartFor(target),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaybackPlanAction {
    ReplacePlan,
    KeepListening,
}

/// Without a live connection there is no connection loop to consume a
/// ReplacePlan command, so a reset must keep the listener instead.
pub(super) fn plan_playback_reset(connection: &ConnectionState) -> PlaybackPlanAction {
    if matches!(connection, ConnectionState::Connected { .. }) {
        PlaybackPlanAction::ReplacePlan
    } else {
        PlaybackPlanAction::KeepListening
    }
}

pub(super) struct InstallContext {
    pub(super) events: UnboundedSender<ClientEvent>,
    pub(super) session: Arc<Mutex<CourseSession>>,
    pub(super) transitions: Arc<ClientTransitionRuntime>,
}

pub(super) struct ProtocolServerSlot {
    server: Mutex<Option<ServerHandle>>,
    watch: Mutex<Option<JoinHandle<()>>>,
}

impl ProtocolServerSlot {
    pub(super) fn new() -> Self {
        Self {
            server: Mutex::new(None),
            watch: Mutex::new(None),
        }
    }

    pub(super) fn connection_state(&self) -> Option<ConnectionState> {
        self.server
            .lock()
            .ok()?
            .as_ref()
            .map(|server| server.state.borrow().clone())
    }

    pub(super) fn playback_state(&self) -> Option<PlaybackState> {
        self.server
            .lock()
            .ok()?
            .as_ref()
            .map(|server| server.playback.borrow().clone())
    }

    pub(super) fn commands(&self) -> Option<PlaybackCommandSender> {
        self.server
            .lock()
            .ok()?
            .as_ref()
            .map(|server| server.commands.clone())
    }

    pub(super) fn connection_watch(&self) -> Option<watch::Receiver<ConnectionState>> {
        self.server
            .lock()
            .ok()?
            .as_ref()
            .map(|server| server.state.clone())
    }

    pub(super) fn playback_and_commands(
        &self,
    ) -> Option<(PlaybackCommandSender, watch::Receiver<PlaybackState>)> {
        let server = self.server.lock().ok()?;
        let server = server.as_ref()?;
        Some((server.commands.clone(), server.playback.clone()))
    }

    pub(super) fn has_control_server(&self, control_port: u16) -> bool {
        self.server
            .lock()
            .ok()
            .and_then(|server| {
                server
                    .as_ref()
                    .map(|server| control_port == 0 || server.port == control_port)
            })
            .unwrap_or(false)
    }

    /// Remove the handle and watch task without shutting either down. Callers
    /// own the returned pair for a course switch.
    pub(super) fn take(&self) -> (Option<JoinHandle<()>>, Option<ServerHandle>) {
        let watch = self.watch.lock().ok().and_then(|mut watch| watch.take());
        let server = self.server.lock().ok().and_then(|mut server| server.take());
        (watch, server)
    }

    /// Commands are only safe to use while the Adapter is actually connected.
    pub(super) fn connected_commands(&self) -> Option<PlaybackCommandSender> {
        let server = self.server.lock().ok()?;
        let server = server.as_ref()?;
        if !matches!(*server.state.borrow(), ConnectionState::Connected { .. }) {
            return None;
        }
        Some(server.commands.clone())
    }

    pub(super) fn adapter(&self) -> Option<AdapterId> {
        let server = self.server.lock().ok()?;
        match server.as_ref()?.adapter_name() {
            "vscode" => Some(AdapterId::Vscode),
            "jupyterlab" => Some(AdapterId::Jupyterlab),
            _ => None,
        }
    }

    pub(super) fn install(
        &self,
        server: ServerHandle,
        replacement: ReplacementKind,
        context: InstallContext,
    ) -> Result<(), String> {
        let mut playback = server.playback.clone();
        let mut connection_watch = server.state.clone();
        let commands = server.commands.clone();
        let InstallContext {
            events,
            session,
            transitions,
        } = context;
        let suppress_initial = replacement == ReplacementKind::Controlled;

        let watch = tokio::spawn(async move {
            let mut last_signature = session.lock().ok().map(|session| session.state_signature());
            let mut last_persisted_step: Option<(String, String)> = None;
            let mut was_connected = matches!(
                *connection_watch.borrow(),
                ConnectionState::Connected { .. }
            );
            // A controlled restart must not publish its transient Waiting state
            // and cancel the Start that is waiting for the handshake. An already
            // connected initial state is still a fact the Client must receive.
            let initial_connection = connection_watch.borrow().clone();
            if !suppress_initial || matches!(initial_connection, ConnectionState::Connected { .. })
            {
                let _ = events.send(ClientEvent::Connection(initial_connection));
            }
            let _ = events.send(ClientEvent::Playback(Box::new(playback.borrow().clone())));
            loop {
                tokio::select! {
                    result = connection_watch.changed() => {
                        if result.is_err() {
                            break;
                        }
                        let connection = connection_watch.borrow().clone();
                        let is_connected =
                            matches!(connection, ConnectionState::Connected { .. });
                        if was_connected && !is_connected {
                            transitions.invalidate_adapter_transitions();
                        }
                        was_connected = is_connected;
                        let _ = events.send(ClientEvent::Connection(connection));
                    }
                    result = playback.changed() => {
                        if result.is_err() {
                            break;
                        }
                        let playback = playback.borrow().clone();
                        let mut course_state = None;
                        // Publish only after the progress write: a listener may
                        // reset the Unit immediately after seeing StepComplete.
                        if let Ok(mut session) = session.lock() {
                            if let Err(error) =
                                persist_playback(&mut session, &playback, &mut last_persisted_step)
                            {
                                let detail = error.to_string();
                                let message =
                                    t_with("error.progress_save_failed", &[("error", &detail)]);
                                let _ = events.send(ClientEvent::Warnings(vec![message.clone().into()]));
                                // The server owns the playback state. Its failure command
                                // cancels the active Action and turns playback into a
                                // retryable error instead of silently advancing.
                                if !matches!(&playback, PlaybackState::Error { .. }) {
                                    let _ = commands.persistence_failed(message);
                                }
                            }
                            let signature = session.state_signature();
                            if last_signature != Some(signature) {
                                last_signature = Some(signature);
                                course_state = Some(session.state());
                            }
                        }
                        let _ = events.send(ClientEvent::Playback(Box::new(playback)));
                        if let Some(state) = course_state {
                            let _ = events.send(ClientEvent::Course(Arc::new(state)));
                        }
                    }
                }
            }
        });

        *self
            .server
            .lock()
            .map_err(|_| t("error.playback_server_unavailable"))? = Some(server);
        *self
            .watch
            .lock()
            .map_err(|_| t("error.playback_watch_unavailable"))? = Some(watch);
        Ok(())
    }

    pub(super) async fn stop(&self) {
        if let Some(task) = self.watch.lock().ok().and_then(|mut watch| watch.take()) {
            task.abort();
        }
        let server = self.server.lock().ok().and_then(|mut server| server.take());
        if let Some(server) = server {
            server.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_with_the_target_adapter_is_reused() {
        assert_eq!(
            plan_server(Some(AdapterId::Vscode), Some(AdapterId::Vscode)),
            ServerPlan::Reuse
        );
        assert_eq!(
            plan_server(Some(AdapterId::Jupyterlab), Some(AdapterId::Jupyterlab)),
            ServerPlan::Reuse
        );
    }

    #[test]
    fn a_different_target_adapter_starts_a_new_server() {
        assert_eq!(
            plan_server(Some(AdapterId::Jupyterlab), Some(AdapterId::Vscode)),
            ServerPlan::StartFor(AdapterId::Vscode)
        );
        assert_eq!(
            plan_server(None, Some(AdapterId::Vscode)),
            ServerPlan::StartFor(AdapterId::Vscode)
        );
    }

    #[test]
    fn a_unit_without_an_adapter_keeps_the_server_listening() {
        assert_eq!(
            plan_server(Some(AdapterId::Jupyterlab), None),
            ServerPlan::KeepListening
        );
    }

    #[test]
    fn a_connected_server_replaces_the_reset_plan() {
        assert_eq!(
            plan_playback_reset(&ConnectionState::Connected {
                adapter: "vscode".to_string(),
            }),
            PlaybackPlanAction::ReplacePlan
        );
    }

    #[test]
    fn a_disconnected_server_keeps_the_reset_listener() {
        assert_eq!(
            plan_playback_reset(&ConnectionState::WaitingForAdapter),
            PlaybackPlanAction::KeepListening
        );
        assert_eq!(
            plan_playback_reset(&ConnectionState::Error {
                code: "invalid_origin".to_string(),
                message: "bad origin".to_string(),
            }),
            PlaybackPlanAction::KeepListening
        );
    }
}
