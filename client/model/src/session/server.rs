//! Protocol Server lifecycle, connection/playback accessors, and waits.

use super::*;

impl SessionService {
    pub(super) fn connection_state(&self) -> ConnectionState {
        self.inner
            .protocol_server
            .connection_state()
            .unwrap_or(ConnectionState::WaitingForAdapter)
    }

    pub(super) fn server_adapter(&self) -> Option<AdapterId> {
        self.inner.protocol_server.adapter()
    }

    pub(super) async fn ensure_protocol_adapter(
        &self,
        unit_path: &str,
        ticket: &ClientTransitionTicket,
    ) -> Result<(), String> {
        let adapter = {
            let session = self
                .inner
                .session
                .lock()
                .map_err(|_| t("error.course_session_unavailable"))?;
            session
                .required_adapters_for_unit(unit_path)
                .first()
                .copied()
        };
        let has_server = self.inner.protocol_server.commands().is_some();
        // A missing server (for example after a failed course switch) is rebuilt
        // for the current course target, including the local/adapter-less case.
        match plan_server(self.server_adapter(), adapter) {
            ServerPlan::KeepListening | ServerPlan::Reuse if has_server => Ok(()),
            ServerPlan::KeepListening | ServerPlan::Reuse => {
                self.restart_server_for_current_course().await?;
                self.wait_for_connection_or_cancel(unit_path, ticket).await
            }
            ServerPlan::StartFor(adapter) => {
                self.restart_server_for_adapter(adapter).await?;
                self.wait_for_connection_or_cancel(unit_path, ticket).await
            }
        }
    }

    pub(super) fn playback_state(&self) -> Result<PlaybackState, String> {
        self.inner
            .protocol_server
            .playback_state()
            .ok_or_else(|| t("error.playback_service_not_started"))
    }

    pub(super) fn server_commands(&self) -> Result<PlaybackCommandSender, String> {
        self.inner
            .protocol_server
            .commands()
            .ok_or_else(|| t("error.playback_service_not_started"))
    }

    /// Who the Protocol server should expect on the other end of the socket.
    ///
    /// Three cases, in the order they can be told apart: a Course that names an
    /// Adapter gets that Adapter; a Course that names none may only contain
    /// `Present` and `Wait`, so the built-in local adapter answers instead
    /// (spec §4.2); with no Course open at all the server keeps waiting for VS
    /// Code, which is what connects first and opens the Course.
    fn current_course_target(&self) -> Result<AdapterTarget, String> {
        let session = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?;
        let local_adapter = self.inner.local_adapter_enabled.load(Ordering::Relaxed);
        Ok(match session.selected_adapter() {
            Some(adapter) => AdapterTarget::External(adapter),
            None if local_adapter && session.active_course().is_some() => AdapterTarget::Local,
            None => AdapterTarget::External(AdapterId::Vscode),
        })
    }

    pub(super) async fn start_server_for_target(
        &self,
        target: AdapterTarget,
        origin: Option<&str>,
    ) -> Result<ServerHandle, String> {
        let (workspace, steps, resume) = {
            let session = self
                .inner
                .session
                .lock()
                .map_err(|_| t("error.course_session_unavailable"))?;
            (
                session.active_workspace().unwrap_or_default(),
                session
                    .try_playback_steps()
                    .map_err(|error| error.to_string())?,
                session.playback_resume(),
            )
        };
        let origin = origin
            .map(str::to_string)
            .or_else(|| self.inner.jupyterlab.origin());
        if target == AdapterTarget::External(AdapterId::Jupyterlab)
            && let Some(origin) = origin
        {
            return start_server_with_path_and_resume_for_adapter_on_port_with_auth(
                workspace,
                self.inner.runtime_path.clone(),
                steps,
                resume,
                target,
                AdapterAuth::JupyterLab { origin },
                self.inner.jupyterlab.control_port(),
            )
            .await
            .map_err(|error| error.to_string());
        }
        start_server_with_path_and_resume_for_adapter(
            workspace,
            self.inner.runtime_path.clone(),
            steps,
            resume,
            target,
        )
        .await
        .map_err(|error| error.to_string())
    }

    pub(super) async fn start_server_for_current_course(&self) -> Result<ServerHandle, String> {
        let target = self.current_course_target()?;
        self.start_server_for_target(target, None).await
    }

    pub(super) async fn replace_server_for_adapter(
        &self,
        adapter: AdapterId,
        replacement: ReplacementKind,
        origin: Option<&str>,
    ) -> Result<(), String> {
        self.replace_server_for_target(AdapterTarget::External(adapter), replacement, origin)
            .await
    }

    pub(super) async fn replace_server_for_target(
        &self,
        target: AdapterTarget,
        replacement: ReplacementKind,
        origin: Option<&str>,
    ) -> Result<(), String> {
        let start = self.start_server_for_target(target, origin);
        self.replace_server(replacement, start).await
    }

    pub(super) async fn restart_server_for_adapter(
        &self,
        adapter: AdapterId,
    ) -> Result<(), String> {
        self.replace_server_for_adapter(adapter, ReplacementKind::Controlled, None)
            .await
    }

    pub(super) async fn restart_server_for_current_course(&self) -> Result<(), String> {
        let target = self.current_course_target()?;
        self.replace_server_for_target(target, ReplacementKind::Controlled, None)
            .await
    }

    pub(super) async fn restart_server_on_runtime(&self) -> Result<(), String> {
        let service = self.clone();
        self.inner
            ._runtime
            .spawn(async move { service.restart_server_for_current_course().await })
            .await
            .map_err(|error| {
                let detail = error.to_string();
                t_with("error.playback_task_failed", &[("error", &detail)])
            })?
    }

    /// Wait for a real Connected event. The Start transition owns the
    /// reconnect window, so there is no fixed grace period and no polling.
    pub(super) async fn wait_for_connection_or_cancel(
        &self,
        unit_path: &str,
        ticket: &ClientTransitionTicket,
    ) -> Result<(), String> {
        let requires_adapter = !self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?
            .required_adapters_for_unit(unit_path)
            .is_empty();
        if !requires_adapter {
            return Ok(());
        }
        let mut connection = self
            .inner
            .protocol_server
            .connection_watch()
            .ok_or_else(|| t("error.playback_service_not_started"))?;
        loop {
            if !self.inner.transitions.transition_is_current(ticket) {
                return Err(t("error.learning_start_cancelled"));
            }
            if let Some(result) = connection_wait_result(&connection.borrow()) {
                return result;
            }
            tokio::select! {
                changed = connection.changed() => {
                    if changed.is_err() {
                        return Err(t("error.playback_server_unavailable"));
                    }
                }
                _ = self.inner.transitions.wait_for_transition_end(ticket) => {
                    return Err(t("error.learning_start_cancelled"));
                }
            }
        }
    }

    pub(super) async fn wait_for_server_connection(&self, wait: Duration) -> bool {
        let Some(mut connection) = self.inner.protocol_server.connection_watch() else {
            return false;
        };
        if matches!(*connection.borrow(), ConnectionState::Connected { .. }) {
            return true;
        }
        let _ = tokio::time::timeout(wait, async {
            loop {
                if matches!(*connection.borrow(), ConnectionState::Connected { .. }) {
                    break;
                }
                if connection.changed().await.is_err() {
                    break;
                }
            }
        })
        .await;
        matches!(*connection.borrow(), ConnectionState::Connected { .. })
    }

    pub(super) fn install_server(
        &self,
        server: ServerHandle,
        replacement: ReplacementKind,
    ) -> Result<(), String> {
        self.inner.protocol_server.install(
            server,
            replacement,
            InstallContext {
                events: self.inner.events.clone(),
                session: self.inner.session.clone(),
                transitions: self.inner.transitions.clone(),
            },
        )
    }

    pub(super) async fn replace_server(
        &self,
        replacement: ReplacementKind,
        start: impl Future<Output = Result<ServerHandle, String>>,
    ) -> Result<(), String> {
        self.stop_server().await;
        let server = start.await?;
        self.install_server(server, replacement)
    }

    pub(super) async fn stop_server(&self) {
        self.inner.protocol_server.stop().await;
    }
}
