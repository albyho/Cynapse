//! WebSocket protocol server: one connection per adapter, playback state
//! broadcast, and step/boundary progression.
//!
//! External API is re-exported at this root (`pub use types::*` and
//! `pub use entry::*`); import from `crate::server::...`, never from
//! submodules directly.
//!
//! Submodules:
//! - `types` — public playback/connection types, commands, errors
//! - `entry` — `start_server*` entry-point family and initial state
//! - `connection` — `handle_connection` protocol loop (single function)
//! - `progression` — step/boundary progression helpers (`pub(super)`)
//! - `local_adapter` — built-in local adapter loop
//! - `playback` — playback session state and command application
//! - `environment_check` — adapter environment validation
//! - `tests` — white-box tests (index in tests.rs)

mod connection;
mod entry;
mod environment_check;
mod local_adapter;
mod playback;
mod progression;
mod types;

pub use entry::*;
pub use types::*;

use playback::{
    ActionDispatchContext, ActionNarrationRuntime, ActiveAction, PlaybackCursor, PlaybackIndex,
    completed_cursor, dispatch_action, first_cursor, next_cursor_in_unit, playback_position,
    restart_step_cursor, resume_cursor, send_action,
};

use crate::course::{
    Action, ActionNarrationPhase, AdapterId, EnvironmentRequirement, PlaybackAction, PlaybackStep,
    TerminalCommandAction, TerminalEnsureAction,
};
use crate::discovery::{DiscoveryError, DiscoveryHandle, new_discovery};
use crate::environment::{
    EnvironmentCheckItem, EnvironmentCheckReport, output_satisfies_minimum_version,
};
use crate::protocol::{
    AckPayload, AckStatus, CancelReason, CheckPayload, Envelope, HelloPayload, MessageType,
    PROTOCOL_VERSION,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use uuid::Uuid;

pub struct ServerHandle {
    pub port: u16,
    /// Per-launch handshake token from the discovery file.
    pub token: String,
    pub runtime_path: PathBuf,
    pub state: watch::Receiver<ConnectionState>,
    pub playback: watch::Receiver<PlaybackState>,
    pub commands: PlaybackCommandSender,
    adapter_name: &'static str,
    discovery: DiscoveryHandle,
    local_adapter_task: Option<tokio::task::JoinHandle<()>>,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

/// Tracks which authenticated connection is newest. A replacement closes the
/// older socket and prevents its cleanup from marking the new tab disconnected.
struct AdapterGeneration {
    generation: u64,
    receiver: watch::Receiver<u64>,
    newest: Arc<std::sync::atomic::AtomicU64>,
}

struct PlaybackSession {
    cursor: Option<PlaybackCursor>,
    state: PlaybackState,
    pending_boundary: Option<PendingBoundary>,
    correlation_id: Option<String>,
    skip_waits: bool,
    skip_waits_through: Option<PlaybackCursor>,
    force_skip_waits: bool,
    skip_unit_final_wait: bool,
}

#[derive(Debug, Clone, Copy)]
struct LocalWait {
    cursor: PlaybackCursor,
    deadline: tokio::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingBoundary {
    Step {
        completed: PlaybackCursor,
        next: PlaybackCursor,
    },
    Unit {
        completed: PlaybackCursor,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryProgression {
    Automatic,
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundaryAdvance {
    NoBoundary,
    Waiting,
    Advanced,
}

fn playback_state_kind(state: &PlaybackState) -> &'static str {
    match state {
        PlaybackState::Idle => "idle",
        PlaybackState::Ready { .. } => "ready",
        PlaybackState::Playing { .. } => "playing",
        PlaybackState::Narrating { .. } => "narrating",
        PlaybackState::Paused { .. } => "paused",
        PlaybackState::StepComplete { .. } => "step_complete",
        PlaybackState::UnitComplete { .. } => "unit_complete",
        PlaybackState::Error { .. } => "error",
    }
}

impl PlaybackSession {
    /// Consume the stored boundary and advance it. A boundary that still needs
    /// an explicit learner response is put back for the next Play command.
    pub(super) fn advance_stored_boundary(
        &mut self,
        steps: &[PlaybackStep],
        index: &PlaybackIndex,
        playback_tx: &watch::Sender<PlaybackState>,
        progression: BoundaryProgression,
    ) -> BoundaryAdvance {
        let Some(boundary) = self.pending_boundary.take() else {
            return BoundaryAdvance::NoBoundary;
        };
        if self.advance_boundary(steps, index, playback_tx, boundary, progression) {
            BoundaryAdvance::Advanced
        } else {
            self.pending_boundary = Some(boundary);
            BoundaryAdvance::Waiting
        }
    }

    fn advance_boundary(
        &mut self,
        steps: &[PlaybackStep],
        index: &PlaybackIndex,
        playback_tx: &watch::Sender<PlaybackState>,
        boundary: PendingBoundary,
        progression: BoundaryProgression,
    ) -> bool {
        self.skip_waits = self.force_skip_waits;
        self.skip_waits_through = None;
        let completed = match boundary {
            PendingBoundary::Step { completed, .. } | PendingBoundary::Unit { completed, .. } => {
                completed
            }
        };
        let completed_position =
            playback_position(steps, index, completed).expect("completed boundary has a position");
        let completed_step = steps.get(completed.step_index);
        let requires_human =
            completed_step.is_some_and(|step| step.requires_human || step.challenge);
        let challenge_checkpoint = completed_step.is_some_and(|step| step.challenge);
        if challenge_checkpoint
            && progression == BoundaryProgression::Automatic
            && matches!(&boundary, PendingBoundary::Unit { .. })
        {
            log::info!(
                target: "cynapse.server",
                "playback.waiting correlation_id={} reason=challenge_checkpoint step_id={} unit_path={} requires_human=true",
                self.correlation_id.as_deref().unwrap_or("-"),
                completed_position.step_id,
                completed_position.unit_path
            );
            self.cursor = None;
            self.state = PlaybackState::StepComplete {
                position: completed_position,
                continuation: None,
                requires_human: true,
            };
            let _ = playback_tx.send(self.state.clone());
            return false;
        }

        match boundary {
            PendingBoundary::Step { next, .. } => {
                let next_step_id = steps
                    .get(next.step_index)
                    .map(|step| step.step_id.as_str())
                    .unwrap_or("-");
                log::info!(
                    target: "cynapse.server",
                    "step.completed correlation_id={} step_id={} unit_path={} next_step_id={} requires_human={} continuation=false",
                    self.correlation_id.as_deref().unwrap_or("-"),
                    completed_position.step_id,
                    completed_position.unit_path,
                    next_step_id,
                    requires_human
                );
                self.cursor = Some(next);
                self.state = PlaybackState::StepComplete {
                    position: completed_position,
                    continuation: None,
                    requires_human,
                };
            }
            PendingBoundary::Unit { .. } => {
                log::info!(
                    target: "cynapse.server",
                    "unit.completed correlation_id={} step_id={} unit_path={} requires_human={}",
                    self.correlation_id.as_deref().unwrap_or("-"),
                    completed_position.step_id,
                    completed_position.unit_path,
                    requires_human
                );
                self.cursor = None;
                self.state = PlaybackState::UnitComplete {
                    position: completed_position,
                };
            }
        }
        let _ = playback_tx.send(self.state.clone());
        true
    }

    pub(super) fn restart_from(&mut self, cursor: Option<PlaybackCursor>) {
        self.cursor = cursor.map(restart_step_cursor);
        self.pending_boundary = None;
    }

    pub(super) fn restart_at(&mut self, cursor: PlaybackCursor) {
        self.restart_from(Some(cursor));
    }

    pub(super) fn interrupted_action(
        &mut self,
        cursor: PlaybackCursor,
        steps: &[PlaybackStep],
        index: &PlaybackIndex,
        interruption: PlaybackInterruption,
        error: Option<(String, String)>,
    ) -> PlaybackState {
        let position = playback_position(steps, index, cursor);
        self.restart_at(cursor);
        if let Some((code, message)) = error {
            PlaybackState::Error {
                code,
                message,
                position,
                interruption: Some(interruption),
            }
        } else {
            PlaybackState::Paused {
                position: position.expect("active cursor has a position"),
                interruption,
            }
        }
    }

    pub(super) fn failed_action(
        &mut self,
        cursor: Option<PlaybackCursor>,
        position: Option<PlaybackPosition>,
        code: String,
        message: String,
        interruption: Option<PlaybackInterruption>,
    ) -> PlaybackState {
        self.restart_from(cursor);
        PlaybackState::Error {
            code,
            message,
            position,
            interruption,
        }
    }
}

struct ServerPlaybackPlan {
    steps: Arc<Vec<PlaybackStep>>,
    index: PlaybackIndex,
    adapter_name: &'static str,
}

impl ServerPlaybackPlan {
    fn new(steps: Vec<PlaybackStep>, adapter_name: &'static str) -> Self {
        let index = PlaybackIndex::new(&steps);
        Self {
            steps: Arc::new(steps),
            index,
            adapter_name,
        }
    }
}

// A connected adapter that never answers must not freeze playback forever.
// Course-declared timeouts are capped at 30 minutes; the extra 30 seconds
// keeps the ACK watchdog behind the adapter's own timeout and grace window.
#[cfg(not(test))]
pub(crate) const ACTION_ACK_TIMEOUT: Duration = Duration::from_secs(30 * 60 + 30);

// Unit tests use the same watchdog path on a short clock so heartbeat timeouts
// do not hide the per-Action deadline behavior.
#[cfg(test)]
pub(crate) const ACTION_ACK_TIMEOUT: Duration = Duration::from_secs(2);

// Ping every 20s; an adapter that misses three pings is treated as gone.
// Ping/Pong is transport-level: a healthy adapter keeps answering while a long
// Action runs, so silence means the connection is dead. A pending ACK deadline
// does not exempt it; that check only lets a timed-out Action report
// `action_timeout` before the heartbeat closes the socket.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(60);

/// Close code telling an older JupyterLab page that a newer page is active.
pub const INACTIVE_ADAPTER_CLOSE_CODE: u16 = 4001;
pub(crate) const INACTIVE_ADAPTER_REASON: &str = "Inactive: a newer JupyterLab page is active";

impl ServerHandle {
    /// Stable protocol identity this server authenticates.
    pub fn adapter_name(&self) -> &'static str {
        self.adapter_name
    }

    pub async fn shutdown(mut self) {
        let _ = self.shutdown.take().map(|shutdown| shutdown.send(()));
        if let Some(task) = self.local_adapter_task.take() {
            task.abort();
        }
        self.task.abort();
        let _ = self.discovery.remove();
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.take().map(|shutdown| shutdown.send(()));
        if let Some(task) = self.local_adapter_task.take() {
            task.abort();
        }
        self.task.abort();
        let _ = self.discovery.remove();
    }
}

fn initial_playback_state(
    plan: &ServerPlaybackPlan,
    resume: Option<&PlaybackResume>,
) -> PlaybackState {
    let cursor = initial_cursor(plan, resume);
    match resume.and_then(|resume| resume.recovery.as_ref()) {
        Some(recovery) => {
            match cursor.and_then(|cursor| playback_position(&plan.steps, &plan.index, cursor)) {
                Some(position) => PlaybackState::Paused {
                    position,
                    interruption: PlaybackInterruption {
                        reason: recovery.reason,
                        exit_code: recovery.exit_code,
                        output: recovery.output.clone(),
                        output_truncated: Some(recovery.output_truncated),
                    },
                },
                None => PlaybackState::Ready { position: None },
            }
        }
        None => PlaybackState::Ready {
            position: cursor.and_then(|cursor| {
                playback_position(&plan.steps, &plan.index, cursor).map(|mut position| {
                    position.step_started = resume.is_some_and(|resume| resume.step_started);
                    position
                })
            }),
        },
    }
}

fn initial_cursor(
    plan: &ServerPlaybackPlan,
    resume: Option<&PlaybackResume>,
) -> Option<PlaybackCursor> {
    resume
        .and_then(|resume| resume_cursor(&plan.steps, resume))
        .or_else(|| first_cursor(&plan.steps))
}

fn resume_skip_waits_through(
    plan: &ServerPlaybackPlan,
    resume: Option<&PlaybackResume>,
) -> Option<PlaybackCursor> {
    let resume = resume?;
    if !resume.step_started {
        return None;
    }
    let action_index = resume.action_index?;
    let cursor = resume_cursor(&plan.steps, resume)?;
    Some(PlaybackCursor {
        step_index: cursor.step_index,
        action_index,
    })
}

#[cfg(test)]
mod tests;
