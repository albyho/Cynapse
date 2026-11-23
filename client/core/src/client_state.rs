use crate::{
    AdapterId, ConnectionState, CourseSessionState, LearningMode, LearningView, PlaybackState,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientTransitionKind {
    Preparing,
    Selecting,
    Starting,
    Stopping,
    Resetting,
    Opening,
    Advancing,
    Retrying,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientTransitionPhase {
    Running,
    Finalizing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientTransitionStage {
    PrepareEnvironment,
    ConnectAdapter,
    PrepareWorkspace,
    CheckEnvironment,
    StartPlayback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientTransitionView {
    pub id: String,
    pub kind: ClientTransitionKind,
    pub phase: ClientTransitionPhase,
    pub cancellable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<ClientTransitionStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingLearningIntent {
    pub unit_path: String,
    pub mode: LearningMode,
}

/// Something the Client has to react to.
///
/// Cross-thread producers (the core watch channels, the tray, single-instance
/// activation, audio completion, a finished JupyterLab start) send these to the
/// app; the app forwards the state facts to `ClientModel::apply`, which hands
/// them to the reducer. Most variants replace state; `OpenUrl` asks for an
/// action instead and reaches the reducer only to be ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    Connection(ConnectionState),
    Course(Arc<CourseSessionState>),
    Learning(LearningView),
    Playback(Box<PlaybackState>),
    Warnings(Vec<Warning>),
    Transition(Option<ClientTransitionView>),
    PendingTransition(Option<ClientTransitionView>),
    PendingLearningIntent(Option<PendingLearningIntent>),
    /// The app opens this URL in the default browser. No state to reduce.
    OpenUrl(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ClientStateData {
    course: Arc<CourseSessionState>,
    learning: LearningView,
    playback: PlaybackState,
    connection: ConnectionState,
    warnings: Vec<Warning>,
    transition: Option<ClientTransitionView>,
    pending_transition: Option<ClientTransitionView>,
    pending_learning_intent: Option<PendingLearningIntent>,
}

#[derive(Debug, Clone, Default)]
pub struct ClientStateReducer {
    data: ClientStateData,
}

impl ClientStateReducer {
    pub fn new(
        course: CourseSessionState,
        learning: LearningView,
        playback: PlaybackState,
        connection: ConnectionState,
    ) -> Self {
        Self {
            data: ClientStateData {
                course: Arc::new(course),
                learning,
                playback,
                connection,
                warnings: Vec::new(),
                transition: None,
                pending_transition: None,
                pending_learning_intent: None,
            },
        }
    }

    /// Applies events and reports whether any field changed.
    pub fn apply(&mut self, events: &[ClientEvent]) -> bool {
        let mut changed = false;
        for event in events {
            match event {
                ClientEvent::Course(course) => {
                    // Course events are emitted only when visible course state
                    // changed; replacing the shared Arc avoids an O(course)
                    // equality scan here.
                    self.data.course = Arc::clone(course);
                    changed = true;
                }
                ClientEvent::Learning(learning) => {
                    if self.data.learning != *learning {
                        self.data.learning = learning.clone();
                        changed = true;
                    }
                }
                ClientEvent::Playback(playback) => {
                    if self.data.playback != **playback {
                        self.data.playback = (**playback).clone();
                        changed = true;
                    }
                }
                ClientEvent::Connection(connection) => {
                    if self.data.connection != *connection {
                        self.data.connection = connection.clone();
                        changed = true;
                    }
                }
                ClientEvent::Warnings(warnings) => {
                    if self.data.warnings != *warnings {
                        self.data.warnings = warnings.clone();
                        changed = true;
                    }
                }
                ClientEvent::Transition(transition) => {
                    if self.data.transition != *transition {
                        self.data.transition = transition.clone();
                        changed = true;
                    }
                }
                ClientEvent::PendingTransition(transition) => {
                    if self.data.pending_transition != *transition {
                        self.data.pending_transition = transition.clone();
                        changed = true;
                    }
                }
                ClientEvent::PendingLearningIntent(intent) => {
                    if self.data.pending_learning_intent != *intent {
                        self.data.pending_learning_intent = intent.clone();
                        changed = true;
                    }
                }
                ClientEvent::OpenUrl(_) => {}
            }
        }
        changed
    }

    pub fn course(&self) -> &CourseSessionState {
        &self.data.course
    }

    pub fn learning(&self) -> &LearningView {
        &self.data.learning
    }

    pub fn playback(&self) -> &PlaybackState {
        &self.data.playback
    }

    pub fn connection(&self) -> &ConnectionState {
        &self.data.connection
    }

    pub fn warnings(&self) -> &[Warning] {
        &self.data.warnings
    }

    pub fn transition(&self) -> Option<&ClientTransitionView> {
        self.data.transition.as_ref()
    }

    pub fn pending_transition(&self) -> Option<&ClientTransitionView> {
        self.data.pending_transition.as_ref()
    }

    pub fn pending_learning_intent(&self) -> Option<&PendingLearningIntent> {
        self.data.pending_learning_intent.as_ref()
    }
}

pub const ADAPTER_DISCONNECTED_KEY: &str = "error.adapter_disconnected";

pub fn adapter_disconnected_message() -> String {
    cynapse_client_i18n::t(ADAPTER_DISCONNECTED_KEY)
}

/// A user-facing notice held in Client State.
///
/// `AdapterDisconnected` keeps the identity of the one warning callers must
/// recognise; its text is produced when it is rendered, so it follows the
/// active locale. `NextUnitPlanUnavailable` marks a Unit advance that committed
/// the next Unit but could not install its playback plan. `Message` carries
/// text that has no stable key — a lower-level error string, for example — and
/// is shown as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    AdapterDisconnected,
    NextUnitPlanUnavailable,
    Message(String),
}

impl Warning {
    pub fn text(&self) -> String {
        match self {
            Warning::AdapterDisconnected => adapter_disconnected_message(),
            Warning::NextUnitPlanUnavailable => {
                cynapse_client_i18n::t("error.next_unit_plan_unavailable")
            }
            Warning::Message(message) => message.clone(),
        }
    }

    pub fn is_adapter_disconnected(&self) -> bool {
        matches!(self, Warning::AdapterDisconnected)
    }
}

impl std::fmt::Display for Warning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.text())
    }
}

impl From<String> for Warning {
    fn from(message: String) -> Self {
        Warning::Message(message)
    }
}

impl From<&str> for Warning {
    fn from(message: &str) -> Self {
        Warning::Message(message.to_string())
    }
}

/// Whether Retry can resume from an actual playback position.
pub fn playback_is_retryable(playback: &PlaybackState) -> bool {
    matches!(
        playback,
        PlaybackState::Error {
            position: Some(_),
            ..
        }
    )
}

/// Shared gate for every action that depends on the Unit's effective Adapters.
///
/// Every declared Adapter must be connected. The `all` check is a
/// forward-compatible invariant until more `AdapterId` variants exist; today
/// only `vscode` can be declared.
pub fn adapter_readiness(
    adapters: &[AdapterId],
    connection: &ConnectionState,
) -> Result<(), Warning> {
    if adapters.is_empty() {
        return Ok(());
    }
    match connection {
        ConnectionState::Connected { adapter }
            if adapters
                .iter()
                .all(|required| adapter == required.connection_name()) =>
        {
            Ok(())
        }
        _ => Err(Warning::AdapterDisconnected),
    }
}

#[cfg(test)]
mod tests;
