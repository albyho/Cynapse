//! Unit/Course reset, interruption, and workspace cleanup.

use super::*;

/// What a reset clears.
#[derive(Clone, Copy)]
enum ResetSubject<'a> {
    Unit(&'a str),
    Course,
}

impl SessionService {
    /// Returns the workspace paths a Unit reset will touch, for the confirmation
    /// dialog. Read-only; does not reset anything.
    pub fn unit_reset_scope(&self, unit_path: &str) -> Result<UnitResetScope, String> {
        let session = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?;
        session
            .reset_scope_for_unit(unit_path)
            .map_err(|error| error.to_string())
    }

    /// Stops any active playback, closes the Unit's editor tabs and terminals,
    /// then resets its progress and workspace. The playback server is left
    /// connected with an empty plan so the next Start rebuilds it normally.
    pub async fn reset_unit(&self, unit_path: &str) -> Result<CourseSessionState, String> {
        self.reset(ResetSubject::Unit(unit_path)).await
    }

    /// Stops playback, closes all editor tabs and terminals, and clears all
    /// Course progress and Unit workspaces.
    pub async fn reset_course(&self) -> Result<CourseSessionState, String> {
        self.reset(ResetSubject::Course).await
    }

    /// The one reset flow. Its subject only decides which transition message
    /// the user sees, which paths get cleaned up, and what the commit clears.
    async fn reset(&self, subject: ResetSubject<'_>) -> Result<CourseSessionState, String> {
        let message = match subject {
            ResetSubject::Unit(_) => t("transition.resetting_unit"),
            ResetSubject::Course => t("transition.resetting_course"),
        };
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Resetting,
            ClientTransitionRequirement::AdapterIndependent,
            message,
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            self.interrupt_playback(CancelReason::CourseSwitch).await?;
            if !self.begin_finalizing_transition(&ticket) {
                return Err(t("error.reset_superseded"));
            }
            let (paths, exclude_paths) = {
                let session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                match subject {
                    ResetSubject::Unit(unit_path) => {
                        let scope = session
                            .reset_scope_for_unit(unit_path)
                            .map_err(|error| error.to_string())?;
                        (scope.workspace_paths, scope.exclude_paths)
                    }
                    ResetSubject::Course => (
                        session
                            .workspace_reset_paths_for_course_reset(true)
                            .map_err(|error| error.to_string())?,
                        Vec::new(),
                    ),
                }
            };
            self.cleanup_workspace(paths, exclude_paths).await?;
            let state = self
                .inner
                .transitions
                .commit_transition(&ticket, || {
                    let mut session = self
                        .inner
                        .session
                        .lock()
                        .map_err(|_| t("error.course_session_unavailable"))?;
                    let outcome = match subject {
                        ResetSubject::Unit(unit_path) => session.reset_unit(unit_path, true),
                        ResetSubject::Course => session.reset_course_with_removals(true),
                    };
                    outcome
                        .map(|outcome| outcome.state)
                        .map_err(|error| error.to_string())
                })
                .map_err(transition_commit_error)?;
            self.publish_course_state(state.clone());
            self.reset_playback_plan().await;
            Ok(state)
        }
        .await;
        self.finish_transition(ticket);
        result
    }

    pub(super) async fn interrupt_playback(&self, reason: CancelReason) -> Result<(), String> {
        let Some((commands, mut playback)) = self.inner.protocol_server.playback_and_commands()
        else {
            return Ok(());
        };
        if !matches!(
            *playback.borrow(),
            PlaybackState::Playing { .. } | PlaybackState::Narrating { .. }
        ) {
            return Ok(());
        }
        commands
            .cancel_with_reason(reason)
            .map_err(|error| error.to_string())?;
        // JupyterLab may interrupt for five seconds and then restart for up to
        // fifteen more before it can acknowledge a cancel. This is a hung
        // Adapter fallback, not the normal cancellation budget.
        let interrupted = self
            .runtime_timeout(Duration::from_secs(30), async {
                loop {
                    if !matches!(
                        *playback.borrow(),
                        PlaybackState::Playing { .. } | PlaybackState::Narrating { .. }
                    ) {
                        break;
                    }
                    if playback.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;
        interrupted
            .map_err(|_| t("error.playback_did_not_stop"))
            .map(|_| ())
    }

    /// Freezes the old playback before an open/close switch. The Adapter cancel
    /// is best-effort: the recovery fact is written locally so navigation never
    /// waits for an ACK or terminal cleanup.
    pub(super) async fn detach_for_course_switch(&self) -> Vec<cynapse_client_core::Warning> {
        let mut warnings = Vec::new();
        self.inner.jupyterlab.stop();
        let (watch_task, server) = self.inner.protocol_server.take();
        if let Some(task) = watch_task {
            task.abort();
        }
        let Some(server) = server else {
            return warnings;
        };
        let playback = server.playback.borrow().clone();
        if let Err(error) = server
            .commands
            .cancel_with_reason(CancelReason::CourseSwitch)
        {
            warnings.push(error.to_string().into());
        }
        if let Err(error) = self.persist_course_switch(&playback) {
            warnings.push(error.into());
        }
        server.shutdown().await;
        let _ = self
            .inner
            .events
            .send(ClientEvent::Playback(Box::new(PlaybackState::Idle)));
        let _ = self
            .inner
            .events
            .send(ClientEvent::Connection(ConnectionState::WaitingForAdapter));
        warnings
    }

    pub(super) fn persist_course_switch(&self, playback: &PlaybackState) -> Result<(), String> {
        let PlaybackState::Playing { position, .. } = playback else {
            return Ok(());
        };
        let mut session = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?;
        persist_interruption(
            &mut session,
            position,
            Some(&PlaybackInterruption {
                reason: PlaybackInterruptionReason::CourseSwitch,
                exit_code: None,
                output: None,
                output_truncated: None,
            }),
        )
    }

    pub(super) async fn cleanup_terminals(&self, paths: Vec<String>) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let Some(commands) = self.inner.protocol_server.connected_commands() else {
            return Ok(());
        };
        let correlation_id = format!(
            "stop-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let terminals = commands
            .terminal_cleanup_correlated(paths, Vec::new(), &correlation_id)
            .map_err(|error| error.to_string())?;
        await_workspace_cleanup(self, terminals, t("cleanup.terminal")).await
    }

    pub(super) async fn cleanup_workspace(
        &self,
        paths: Vec<String>,
        exclude_paths: Vec<String>,
    ) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let Some(commands) = self.inner.protocol_server.connected_commands() else {
            return Ok(());
        };
        let correlation_id = format!(
            "reset-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let terminals = commands
            .terminal_cleanup_correlated(paths.clone(), exclude_paths.clone(), &correlation_id)
            .map_err(|error| error.to_string())?;
        await_workspace_cleanup(self, terminals, t("cleanup.terminal")).await?;
        let tabs = commands
            .workspace_reset_correlated(paths, exclude_paths, &correlation_id)
            .map_err(|error| error.to_string())?;
        await_workspace_cleanup(self, tabs, t("cleanup.editor_tabs")).await
    }

    pub(super) async fn reset_playback_plan(&self) {
        let Some(commands) = self.inner.protocol_server.commands() else {
            return;
        };
        let connection = self.connection_state();
        if matches!(
            plan_playback_reset(&connection),
            PlaybackPlanAction::ReplacePlan
        ) {
            let success = match commands.replace_plan(Vec::new(), None) {
                Ok(receiver) => matches!(
                    self.runtime_timeout(PLAYBACK_RESET_TIMEOUT, receiver).await,
                    Ok(Ok(Ok(())))
                ),
                Err(_) => false,
            };
            if success {
                return;
            }
            self.stop_server().await;
        }
        // Without a connected Adapter there is no connection loop to consume
        // ReplacePlan. Keep the listener and discovery file so the next
        // Adapter connection can still reach this Server.
        let _ = self
            .inner
            .events
            .send(ClientEvent::Playback(Box::new(PlaybackState::Idle)));
        let _ = self
            .inner
            .events
            .send(ClientEvent::Connection(ConnectionState::WaitingForAdapter));
    }

    pub(super) fn publish_course_state(&self, state: CourseSessionState) {
        let _ = self.inner.events.send(ClientEvent::Course(Arc::new(state)));
    }
}
