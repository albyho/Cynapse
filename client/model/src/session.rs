//! Application-side owner of the Client Core session and protocol server.
//!
//! This is deliberately UI-free. It owns the Tokio runtime, the SQLite-backed
//! `CourseSession`, the active `ServerHandle`, and the watch-channel forwarder
//! that turns core state into `ClientEvent`s.
//!
//! Submodules:
//! - `course` — open, select, and close Course use cases
//! - `jupyterlab` — JupyterLab startup preparation at the SessionService boundary
//! - `jupyterlab_supply` — JupyterLab supply, reuse, origin, and stop state
//! - `learning` — Start/Stop, Step/Unit advance, and Challenge checks
//! - `protocol_server` — active Protocol Server slot and install seam
//! - `reset` — Unit/Course reset, interruption, and workspace cleanup
//! - `server` — server lifecycle, connection/playback accessors, and waits

use super::{ChallengeCheckResult, ChallengeOutcome, ClientEvent};
use cynapse_client_core::{
    AckPayload, AckStatus, ActionNarrationAudioView, AdapterAuth, AdapterId, AdapterTarget,
    CancelReason, ChallengeStatus, CheckPayload, ClientTransitionKind, ClientTransitionPhase,
    ClientTransitionRequirement, ClientTransitionRuntime, ClientTransitionStage,
    ClientTransitionTicket, ClientTransitionView, CommandExecutionStart, CommandInterruption,
    ConnectionState, Course, CourseSession, CourseSessionState, DEFAULT_CONTROL_PORT,
    EnvironmentRequirement, JupyterLabProvisioner, LearningMode, NarrationClipRef,
    PlaybackCommandSender, PlaybackInterruption, PlaybackInterruptionReason, PlaybackPosition,
    PlaybackState, ServerHandle, StopTransitionRequest, SystemJupyterLabProvisioner,
    TransitionCommitError, TransitionRequest, TransitionStartError, TransitionWaitOutcome,
    UnitResetScope, adapter_readiness as check_adapter_readiness, global_runtime_dir,
    global_runtime_path, global_state_db_path, playback_is_retryable,
    start_server_with_path_and_resume_for_adapter,
    start_server_with_path_and_resume_for_adapter_on_port_with_auth, unit_session_key,
};
use cynapse_client_i18n::{t, t_with};
use jupyterlab_supply::JupyterLabSupply;
use protocol_server::{
    InstallContext, PlaybackPlanAction, ProtocolServerSlot, ReplacementKind, ServerPlan,
    plan_playback_reset, plan_server,
};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedSender;

mod course;
mod jupyterlab;
mod jupyterlab_supply;
mod learning;
mod protocol_server;
mod reset;
mod server;

#[cfg(not(test))]
const CHECK_TIMEOUT_GRACE: Duration = Duration::from_secs(5);
#[cfg(test)]
const CHECK_TIMEOUT_GRACE: Duration = Duration::from_millis(100);

#[cfg(not(test))]
const NEXT_UNIT_PLAN_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const NEXT_UNIT_PLAN_TIMEOUT: Duration = Duration::from_millis(100);

#[cfg(not(test))]
const PLAYBACK_RESET_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const PLAYBACK_RESET_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct SessionService {
    inner: Arc<Inner>,
}

struct Inner {
    _runtime: Runtime,
    session: Arc<Mutex<CourseSession>>,
    protocol_server: ProtocolServerSlot,
    events: UnboundedSender<ClientEvent>,
    runtime_path: PathBuf,
    transitions: Arc<ClientTransitionRuntime>,
    jupyterlab: JupyterLabSupply,
    jupyterlab_handshake_timeout: Duration,
    /// Whether an Adapter-less Course is served by the built-in local
    /// adapter. Mutable only through the `suppress_local_adapter` test seam.
    local_adapter_enabled: AtomicBool,
}

fn should_republish_start_connection(
    finished_kind: Option<ClientTransitionKind>,
    connection: &ConnectionState,
) -> bool {
    finished_kind == Some(ClientTransitionKind::Starting)
        && !matches!(connection, ConnectionState::Connected { .. })
}

/// Maps the connection watch to the Start gate. `None` means keep waiting.
fn connection_wait_result(state: &ConnectionState) -> Option<Result<(), String>> {
    match state {
        ConnectionState::Connected { .. } => Some(Ok(())),
        ConnectionState::Error { code, message } => {
            log::warn!(
                target: "cynapse.server",
                "start.adapter_error code={} message={}",
                code,
                message
            );
            Some(Err(message.clone()))
        }
        ConnectionState::WaitingForAdapter => None,
    }
}

impl SessionService {
    pub fn new(events: UnboundedSender<ClientEvent>) -> Result<Self, String> {
        Self::build(events, global_state_db_path(), global_runtime_path())
    }

    pub fn record_resume_action(
        &self,
        unit_path: &str,
        step_id: &str,
        action_index: usize,
    ) -> Result<(), String> {
        let mut session = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?;
        session
            .record_resume_action(unit_path, step_id, action_index)
            .map_err(|error| error.to_string())
    }

    /// Test seam: use an isolated state database and runtime discovery file.
    /// The JupyterLab runtime directory is isolated too, so tests never read or
    /// terminate the developer's real stale-server marker.
    #[doc(hidden)]
    pub fn new_at(events: UnboundedSender<ClientEvent>, db_path: PathBuf) -> Result<Self, String> {
        let runtime_path = db_path.with_file_name("runtime.json");
        let runtime_dir = db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::build_with_jupyterlab(
            events,
            db_path,
            runtime_path,
            Arc::new(SystemJupyterLabProvisioner::new(runtime_dir)),
            DEFAULT_CONTROL_PORT,
            Duration::from_secs(30),
        )
    }

    /// Test seam: a fake JupyterLab provisioner, control port, and handshake
    /// budget.
    #[doc(hidden)]
    pub fn new_at_with_jupyterlab(
        events: UnboundedSender<ClientEvent>,
        db_path: PathBuf,
        provisioner: Arc<dyn JupyterLabProvisioner>,
        control_port: u16,
        handshake_timeout: Duration,
    ) -> Result<Self, String> {
        let runtime_path = db_path.with_file_name("runtime.json");
        Self::build_with_jupyterlab(
            events,
            db_path,
            runtime_path,
            provisioner,
            control_port,
            handshake_timeout,
        )
    }

    /// Test seam: keep headless UI tests deterministic. A live local adapter
    /// connects from a background thread and answers real Actions, and the
    /// events it produces wake the UI test scheduler from the wrong thread.
    /// Suppressed, an Adapter-less Course keeps waiting for VS Code, as it
    /// did before the local adapter existed. Production never calls this.
    #[doc(hidden)]
    pub fn suppress_local_adapter(&self) {
        self.inner
            .local_adapter_enabled
            .store(false, Ordering::Relaxed);
    }

    fn build(
        events: UnboundedSender<ClientEvent>,
        db_path: PathBuf,
        runtime_path: PathBuf,
    ) -> Result<Self, String> {
        Self::build_with_jupyterlab(
            events,
            db_path,
            runtime_path,
            Arc::new(SystemJupyterLabProvisioner::new(global_runtime_dir())),
            DEFAULT_CONTROL_PORT,
            Duration::from_secs(30),
        )
    }

    fn build_with_jupyterlab(
        events: UnboundedSender<ClientEvent>,
        db_path: PathBuf,
        runtime_path: PathBuf,
        jupyterlab_provisioner: Arc<dyn JupyterLabProvisioner>,
        jupyterlab_control_port: u16,
        jupyterlab_handshake_timeout: Duration,
    ) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| {
                let detail = error.to_string();
                t_with("error.client_runtime_start_failed", &[("error", &detail)])
            })?;
        let session = CourseSession::new(db_path).map_err(|error| {
            let detail = error.to_string();
            t_with("error.course_progress_open_failed", &[("error", &detail)])
        })?;
        let service = Self {
            inner: Arc::new(Inner {
                _runtime: runtime,
                session: Arc::new(Mutex::new(session)),
                protocol_server: ProtocolServerSlot::new(),
                events,
                runtime_path,
                transitions: Arc::new(ClientTransitionRuntime::default()),
                jupyterlab: JupyterLabSupply::new(jupyterlab_provisioner, jupyterlab_control_port),
                jupyterlab_handshake_timeout,
                local_adapter_enabled: AtomicBool::new(true),
            }),
        };
        service.inner.jupyterlab.cleanup_stale();
        Ok(service)
    }

    /// Starts the local protocol server as soon as the Client starts, even
    /// before a course is open. Adapters can then connect and open the restored
    /// course workspace while the learner is still choosing what to do.
    pub fn start_server_at_startup(&self) -> Result<(), String> {
        self.inner._runtime.block_on(async {
            let server = self.start_server_for_current_course().await?;
            self.install_server(server, ReplacementKind::External)
        })
    }

    fn try_start_transition(
        &self,
        kind: ClientTransitionKind,
        message: impl Into<String>,
    ) -> Result<ClientTransitionTicket, String> {
        let ticket = self
            .inner
            .transitions
            .try_transition(ClientTransitionRequirement::AdapterIndependent)
            .map_err(transition_start_error)?;
        self.show_transition(&ticket, kind, message.into());
        Ok(ticket)
    }

    fn start_latest_transition(
        &self,
        kind: ClientTransitionKind,
        requirement: ClientTransitionRequirement,
        message: impl Into<String>,
    ) -> Result<ClientTransitionTicket, String> {
        let message = message.into();
        let request = self
            .inner
            .transitions
            .request_latest_transition(requirement)
            .map_err(transition_start_error)?;
        let ticket = match request {
            TransitionRequest::Started(ticket) | TransitionRequest::Queued(ticket) => ticket,
        };
        self.show_transition(&ticket, kind, message);
        Ok(ticket)
    }

    fn show_transition(
        &self,
        ticket: &ClientTransitionTicket,
        kind: ClientTransitionKind,
        message: String,
    ) {
        self.inner.transitions.attach_view(
            ticket,
            ClientTransitionView {
                id: ticket.id.clone(),
                kind,
                phase: ClientTransitionPhase::Running,
                cancellable: true,
                stage: None,
                message: Some(message),
            },
        );
        self.publish_transition_views();
    }

    fn update_starting_stage(&self, stage: ClientTransitionStage, message: impl Into<String>) {
        if self.inner.transitions.set_stage(stage, message.into()) {
            self.publish_transition_views();
        }
    }

    fn finish_transition(&self, ticket: ClientTransitionTicket) {
        let finished_kind = self
            .inner
            .transitions
            .finish_transition(&ticket)
            .map(|view| view.kind);
        self.publish_transition_views();
        // A Start owns its briefly disconnected Adapter. Re-publish only a
        // final non-connected state so a permanent loss is handled normally
        // without faking a reconnect that clears warnings.
        if finished_kind == Some(ClientTransitionKind::Starting) {
            let connection = self.connection_state();
            if should_republish_start_connection(finished_kind, &connection) {
                let _ = self.inner.events.send(ClientEvent::Connection(connection));
            }
        }
    }

    fn begin_stop_transition(&self) -> Option<ClientTransitionTicket> {
        match self.inner.transitions.request_stop() {
            StopTransitionRequest::Started(ticket) => {
                self.inner.transitions.attach_view(
                    &ticket,
                    ClientTransitionView {
                        id: ticket.id.clone(),
                        kind: ClientTransitionKind::Stopping,
                        phase: ClientTransitionPhase::Finalizing,
                        cancellable: false,
                        stage: None,
                        message: Some(t("transition.stopping")),
                    },
                );
                self.publish_transition_views();
                Some(ticket)
            }
            StopTransitionRequest::Joined(_) => None,
        }
    }

    fn finish_stop_transition(&self, ticket: ClientTransitionTicket) {
        self.inner.transitions.finish_stop(&ticket);
        self.publish_transition_views();
    }

    fn begin_finalizing_transition(&self, ticket: &ClientTransitionTicket) -> bool {
        if !self.inner.transitions.begin_finalizing(ticket) {
            return false;
        }
        self.publish_transition_views();
        true
    }

    async fn wait_for_transition(&self, ticket: &ClientTransitionTicket) -> Result<(), String> {
        match self.inner.transitions.wait_for_transition(ticket).await {
            TransitionWaitOutcome::Started => Ok(()),
            TransitionWaitOutcome::Superseded => {
                self.finish_transition(ticket.clone());
                Err(t("error.operation_superseded"))
            }
            TransitionWaitOutcome::Cancelled => {
                self.finish_transition(ticket.clone());
                Err(t("error.operation_cancelled"))
            }
        }
    }

    fn publish_transition_views(&self) {
        let views = self.inner.transitions.views();
        let _ = self
            .inner
            .events
            .send(ClientEvent::Transition(views.current));
        let _ = self
            .inner
            .events
            .send(ClientEvent::PendingTransition(views.pending));
    }

    pub fn course_state(&self) -> Result<CourseSessionState, String> {
        let session = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?;
        Ok(session.state())
    }

    pub async fn cancel_for_exit(&self) -> Result<(), String> {
        let _guard = self.inner._runtime.enter();
        let result = self.interrupt_playback(CancelReason::ClientExit).await;
        self.inner.jupyterlab.stop();
        result
    }

    /// Creates a Tokio timeout while the SessionService runtime is entered.
    /// The returned `Timeout` future is `Send`, so reset operations can run on
    /// the GPUI background executor without an `EnterGuard` across an await.
    fn runtime_timeout<F>(&self, duration: Duration, future: F) -> tokio::time::Timeout<F>
    where
        F: Future,
    {
        let _guard = self.inner._runtime.enter();
        tokio::time::timeout(duration, future)
    }

    pub fn narration_audio(
        &self,
        position: &PlaybackPosition,
        clip: &NarrationClipRef,
    ) -> Result<Option<ActionNarrationAudioView>, String> {
        let session = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?;
        session
            .action_narration_audio(
                &position.unit_path,
                &position.step_id,
                clip.action_index,
                clip.phase,
                clip.clip_index,
            )
            .map_err(|error| error.to_string())
    }

    pub fn narration_started(&self, clip: NarrationClipRef) -> Result<(), String> {
        self.inner
            .protocol_server
            .commands()
            .ok_or_else(|| t("error.playback_server_not_ready"))?
            .narration_started(clip)
            .map_err(|error| error.to_string())
    }

    pub fn narration_ended(&self, clip: NarrationClipRef) -> Result<(), String> {
        self.inner
            .protocol_server
            .commands()
            .ok_or_else(|| t("error.playback_server_not_ready"))?
            .narration_ended(clip)
            .map_err(|error| error.to_string())
    }

    pub fn narration_failed(&self, clip: NarrationClipRef, message: String) -> Result<(), String> {
        self.inner
            .protocol_server
            .commands()
            .ok_or_else(|| t("error.playback_server_not_ready"))?
            .narration_failed(clip, message)
            .map_err(|error| error.to_string())
    }
}

async fn await_workspace_cleanup(
    service: &SessionService,
    receiver: tokio::sync::oneshot::Receiver<Result<(), String>>,
    label: String,
) -> Result<(), String> {
    match service
        .runtime_timeout(Duration::from_secs(5), receiver)
        .await
    {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => {
            let detail = error.to_string();
            Err(t_with(
                "error.cleanup_failed",
                &[("label", &label), ("error", &detail)],
            ))
        }
        Ok(Err(_)) => Err(t_with("error.cleanup_interrupted", &[("label", &label)])),
        Err(_) => Err(t_with("error.cleanup_timeout", &[("label", &label)])),
    }
}

fn ensure_checkpoint(
    playback: &PlaybackState,
    unit_path: &str,
    step_id: &str,
) -> Result<(), String> {
    match playback {
        PlaybackState::StepComplete { position, .. }
            if position.unit_path == unit_path && position.step_id == step_id =>
        {
            Ok(())
        }
        _ => Err(t("error.not_at_challenge_checkpoint")),
    }
}

fn is_check_failure(ack: &AckPayload) -> bool {
    ack.code.as_deref().is_some_and(|code| {
        code == "terminal_exit_code_rejected"
            || code == "terminal_output_mismatch"
            || code == "terminal_timeout"
    })
}

/// Persists Step completion and terminal command recovery from playback facts.
///
/// A Challenge Step is only complete after the Challenge passes or is skipped;
/// reaching its boundary alone must not advance persisted progress.
///
/// `last_persisted_step` suppresses repeated cursor writes for the same Step.
/// Recovery replays a Step from its first Action, so per-Action and per-narration
/// positions do not need separate transactions.
///
/// Returns the first database error so the caller can stop playback instead of
/// letting the UI advance past progress that was never saved.
fn persist_playback(
    session: &mut CourseSession,
    playback: &PlaybackState,
    last_persisted_step: &mut Option<(String, String)>,
) -> Result<(), String> {
    let Some(position) = playback_position(playback) else {
        *last_persisted_step = None;
        return Ok(());
    };
    if session.current_unit_path() != Some(position.unit_path.as_str()) {
        *last_persisted_step = None;
        return Ok(());
    }
    let step_key = (position.unit_path.clone(), position.step_id.clone());
    let step_not_persisted = last_persisted_step.as_ref() != Some(&step_key);
    match playback {
        PlaybackState::Playing { position, .. } => {
            if step_not_persisted {
                session
                    .record_progress(
                        &position.unit_path,
                        &position.step_id,
                        step_completed(session, position, false),
                    )
                    .map_err(|error| error.to_string())?;
                *last_persisted_step = Some(step_key);
            }
            // Command recovery is deliberately per Action: a crash must know
            // which terminal command was running, not just which Step was open.
            session
                .complete_running_commands_except(
                    &position.unit_path,
                    &position.step_id,
                    position.action_index,
                )
                .map_err(|error| error.to_string())?;
            record_command_started(session, position)?;
        }
        PlaybackState::Narrating { position, .. } => {
            // Narration belongs to the already-persisted Step; only a Step
            // change needs a cursor/started update.
            if step_not_persisted {
                session
                    .record_progress(
                        &position.unit_path,
                        &position.step_id,
                        step_completed(session, position, false),
                    )
                    .map_err(|error| error.to_string())?;
                *last_persisted_step = Some(step_key);
            }
        }
        PlaybackState::Paused {
            position,
            interruption,
        } => {
            // Interruption recovery is a boundary fact and must be written even
            // when the Step start was already persisted.
            session
                .record_progress(
                    &position.unit_path,
                    &position.step_id,
                    step_completed(session, position, false),
                )
                .map_err(|error| error.to_string())?;
            *last_persisted_step = Some(step_key);
            persist_interruption(session, position, Some(interruption))?;
        }
        PlaybackState::StepComplete { position, .. } | PlaybackState::UnitComplete { position } => {
            // Completion must always land even when the Step start was already
            // persisted by Playing/Narrating.
            session
                .record_progress(
                    &position.unit_path,
                    &position.step_id,
                    step_completed(session, position, true),
                )
                .map_err(|error| error.to_string())?;
            *last_persisted_step = Some(step_key);
            session
                .complete_all_running_commands()
                .map_err(|error| error.to_string())?;
        }
        PlaybackState::Error {
            position: Some(position),
            interruption,
            ..
        } => {
            // Error recovery is a boundary fact and must be written even when
            // the Step start was already persisted.
            session
                .record_progress(
                    &position.unit_path,
                    &position.step_id,
                    step_completed(session, position, false),
                )
                .map_err(|error| error.to_string())?;
            *last_persisted_step = Some(step_key);
            persist_interruption(session, position, interruption.as_ref())?;
        }
        PlaybackState::Idle
        | PlaybackState::Ready { .. }
        | PlaybackState::Error { position: None, .. } => {
            *last_persisted_step = None;
        }
    }
    Ok(())
}

fn playback_position(playback: &PlaybackState) -> Option<&PlaybackPosition> {
    match playback {
        PlaybackState::Playing { position, .. }
        | PlaybackState::Narrating { position, .. }
        | PlaybackState::Paused { position, .. }
        | PlaybackState::StepComplete { position, .. }
        | PlaybackState::UnitComplete { position } => Some(position),
        PlaybackState::Ready { position } => position.as_ref(),
        PlaybackState::Error { position, .. } => position.as_ref(),
        PlaybackState::Idle => None,
    }
}

fn step_completed(
    session: &CourseSession,
    position: &PlaybackPosition,
    default_completed: bool,
) -> bool {
    if session
        .challenge(&position.unit_path, &position.step_id)
        .is_none()
    {
        return default_completed;
    }
    matches!(
        session.challenge_status(&position.unit_path, &position.step_id),
        Some(ChallengeStatus::Passed | ChallengeStatus::Skipped)
    )
}

fn persist_interruption(
    session: &mut CourseSession,
    position: &PlaybackPosition,
    interruption: Option<&PlaybackInterruption>,
) -> Result<(), String> {
    let Some(interruption) = interruption else {
        return Ok(());
    };
    let Some(command) = terminal_command_at(session, position) else {
        return Ok(());
    };
    let (command_text, cwd, timeout_ms) = (
        command.command.clone(),
        command.cwd.clone(),
        command.timeout_ms,
    );
    session
        .record_command_started(CommandExecutionStart {
            unit_path: &position.unit_path,
            step_id: &position.step_id,
            action_index: position.action_index,
            command: &command_text,
            cwd: &cwd,
            timeout_ms,
        })
        .map_err(|error| error.to_string())?;
    session
        .record_command_interrupted(CommandInterruption {
            unit_path: position.unit_path.clone(),
            step_id: position.step_id.clone(),
            action_index: position.action_index,
            reason: interruption.reason,
            exit_code: interruption.exit_code,
            output: interruption.output.clone(),
            output_truncated: interruption.output_truncated.unwrap_or(false),
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn record_command_started(
    session: &mut CourseSession,
    position: &PlaybackPosition,
) -> Result<(), String> {
    let Some(command) = terminal_command_at(session, position) else {
        return Ok(());
    };
    let (command_text, cwd, timeout_ms) = (
        command.command.clone(),
        command.cwd.clone(),
        command.timeout_ms,
    );
    session
        .record_command_started(CommandExecutionStart {
            unit_path: &position.unit_path,
            step_id: &position.step_id,
            action_index: position.action_index,
            command: &command_text,
            cwd: &cwd,
            timeout_ms,
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn terminal_command_at<'a>(
    session: &'a CourseSession,
    position: &PlaybackPosition,
) -> Option<&'a cynapse_client_core::TerminalCommandAction> {
    session.terminal_command_at(
        &position.unit_path,
        &position.step_id,
        position.action_index,
    )
}

fn transition_start_error(error: TransitionStartError) -> String {
    match error {
        TransitionStartError::InProgress => t("error.transition_in_progress"),
        TransitionStartError::StopInProgress => t("error.stop_in_progress"),
    }
}

fn transition_commit_error(error: TransitionCommitError<String>) -> String {
    match error {
        TransitionCommitError::Superseded => t("error.operation_superseded"),
        TransitionCommitError::CommitFailed(error) => error,
    }
}

#[cfg(test)]
mod tests;
