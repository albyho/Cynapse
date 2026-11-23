use super::*;

use super::runner::successful_ack;

pub(super) type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioMode {
    ByStep,
    Continuous,
    StopAfterStep,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdapterReply {
    Acknowledge { payload: AckPayload },
    Cancel,
    Disconnect,
    Timeout,
}

impl Default for AdapterReply {
    fn default() -> Self {
        Self::Acknowledge {
            payload: successful_ack(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdapterScript {
    #[serde(default)]
    pub replies: Vec<AdapterReply>,
    #[serde(default)]
    pub check_replies: Vec<AdapterReply>,
    #[serde(default)]
    pub default_reply: AdapterReply,
    #[serde(default)]
    pub narration: NarrationReply,
}

impl AdapterScript {
    pub fn acknowledge_all() -> Self {
        Self::default()
    }

    pub(super) fn reply_for(&self, action_index: usize) -> AdapterReply {
        self.replies
            .get(action_index)
            .cloned()
            .unwrap_or_else(|| self.default_reply.clone())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NarrationReply {
    #[default]
    Complete,
    EndAfterActionAck,
    Disconnect,
    Timeout,
    Fail,
    FailOnce,
}

#[derive(Debug, Clone)]
pub struct ScenarioSpec {
    pub course_path: PathBuf,
    pub unit_path: Option<String>,
    pub mode: ScenarioMode,
    pub adapter_script: AdapterScript,
    pub adapter_script_path: Option<PathBuf>,
    pub artifact_root: PathBuf,
    pub response_timeout: Duration,
}

/// Execution options that do not change what the scenario reports.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScenarioOptions {
    /// Skips every `Wait` delay. Content verification checks that the Action
    /// is structurally valid but does not spend its authored duration.
    pub skip_waits: bool,
    /// Skips the Challenge after its first failed Check instead of retrying it.
    pub skip_challenge: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioStatus {
    Completed,
    WaitingForHuman,
    Stopped,
    Cancelled,
    TimedOut,
    AdapterDisconnected,
    PlaybackError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioEvent {
    pub sequence: u64,
    pub elapsed_ms: u128,
    pub correlation_id: String,
    pub kind: ScenarioEventKind,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioEventKind {
    ScenarioStarted,
    WorkspacePrepared,
    WorkspaceFilesCaptured,
    AdapterConnected,
    AdapterMessageReceived,
    ActionReceived,
    AdapterMessageSent,
    AdapterDisconnected,
    AdapterTimeout,
    PlaybackState,
    PlaybackError,
    PlaybackPaused,
    HumanStepReached,
    LearningState,
    StopCompleted,
    ResumeCaptured,
    ContinuousUnitReset,
    ContinuousUnitAdvanced,
    NarrationCompleted,
    ScenarioError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterMessage {
    pub correlation_id: String,
    pub direction: AdapterDirection,
    pub envelope: Envelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterDirection {
    Received,
    Sent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioReport {
    pub correlation_id: String,
    pub status: ScenarioStatus,
    pub course_state: cynapse_client_core::course_session::CourseSessionState,
    pub playback_state: PlaybackState,
    pub events: Vec<ScenarioEvent>,
    pub adapter_messages: Vec<AdapterMessage>,
    pub workspace_files: Vec<String>,
    /// Text contents of the final workspace files (path -> content), so
    /// tests can compare terminal workspaces even though completed
    /// scenarios delete their artifact directories. Runtime resources are
    /// omitted because they are a derived copy; files over the per-file or
    /// total size limit are listed in `workspace_contents_skipped`.
    pub workspace_contents: std::collections::BTreeMap<String, String>,
    /// Files omitted from `workspace_contents` because they exceed the
    /// content-capture size limits. Always present; empty means no file was
    /// skipped for size.
    pub workspace_contents_skipped: Vec<String>,
    pub artifact_path: String,
    pub logs_path: String,
    pub artifacts_retained: bool,
    pub repro_command: String,
    pub cleanup_command: String,
}

#[derive(Debug)]
pub enum ScenarioError {
    Io(std::io::Error),
    Server(ServerError),
    Course(cynapse_client_core::course_session::CourseSessionError),
    Protocol(String),
    Execution {
        message: String,
        artifact_path: String,
        repro_command: String,
        cleanup_command: String,
    },
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "scenario I/O error: {error}"),
            Self::Server(error) => write!(formatter, "scenario server error: {error}"),
            Self::Course(error) => write!(formatter, "scenario course error: {error}"),
            Self::Protocol(message) => write!(formatter, "scenario protocol error: {message}"),
            Self::Execution {
                message,
                artifact_path,
                repro_command,
                cleanup_command,
            } => write!(
                formatter,
                "{message}\nartifacts retained at: {artifact_path}\nreproduce with: {repro_command}\ncleanup with: {cleanup_command}"
            ),
        }
    }
}

impl std::error::Error for ScenarioError {}

impl From<std::io::Error> for ScenarioError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ServerError> for ScenarioError {
    fn from(error: ServerError) -> Self {
        Self::Server(error)
    }
}

impl From<cynapse_client_core::course_session::CourseSessionError> for ScenarioError {
    fn from(error: cynapse_client_core::course_session::CourseSessionError) -> Self {
        Self::Course(error)
    }
}

impl From<serde_json::Error> for ScenarioError {
    fn from(error: serde_json::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for ScenarioError {
    fn from(error: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}
