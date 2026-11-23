use super::*;

/// Who answers the Protocol on the other end of the socket.
///
/// A node whose effective `adapters` is empty may only contain `Present` and
/// `Wait` (spec §4.2), and `Present` is answered by the built-in local adapter
/// rather than by an external App. The two cases are a value rather than a
/// boolean plus a name string so the name, the auth mode, and whether the local
/// loop is spawned cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterTarget {
    /// An external editor connects and reports this identity in `hello_ack`.
    External(AdapterId),
    /// The built-in local adapter answers `Present`, rejects every other
    /// Action, and acknowledges `WorkspaceReset`. Nothing external connects.
    Local,
}

/// How an adapter connection proves it belongs to this Client. The two modes
/// stay explicit so the JupyterLab Origin path cannot weaken VS Code's token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterAuth {
    /// VS Code: the per-launch handshake token from `runtime.json`.
    Token,
    /// JupyterLab: the browser `Origin` must match the launched JupyterLab.
    JupyterLab { origin: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    WaitingForAdapter,
    Connected {
        adapter: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlaybackPosition {
    pub parent_id: String,
    pub parent_title: String,
    pub unit_path: String,
    pub unit_title: String,
    pub step_id: String,
    pub step_title: String,
    pub step_started: bool,
    pub step_index: usize,
    pub step_total: usize,
    pub unit_step_index: usize,
    pub unit_step_total: usize,
    pub action_index: usize,
    pub action_total: usize,
    pub overall_action_index: usize,
    pub overall_action_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaybackInterruption {
    pub reason: PlaybackInterruptionReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_truncated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NarrationClipRef {
    pub step_run_id: u64,
    pub action_index: usize,
    pub phase: ActionNarrationPhase,
    pub clip_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NarrationClipContinuation {
    pub clip: NarrationClipRef,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackInterruptionReason {
    UserStop,
    ClientExit,
    CourseSwitch,
    AdapterDisconnected,
    Timeout,
    CommandFailure,
    MediaFailure,
    ForcedTermination,
    #[serde(rename = "jupyterlab_kernel_restarted")]
    JupyterLabKernelRestarted,
}

impl PlaybackInterruptionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserStop => "user_stop",
            Self::ClientExit => "client_exit",
            Self::CourseSwitch => "course_switch",
            Self::AdapterDisconnected => "adapter_disconnected",
            Self::Timeout => "timeout",
            Self::CommandFailure => "command_failure",
            Self::MediaFailure => "media_failure",
            Self::ForcedTermination => "forced_termination",
            Self::JupyterLabKernelRestarted => "jupyterlab_kernel_restarted",
        }
    }

    pub fn from_storage(value: &str) -> Self {
        match value {
            "user_stop" | "user_pause" => Self::UserStop,
            "client_exit" => Self::ClientExit,
            "course_switch" => Self::CourseSwitch,
            "adapter_disconnected" => Self::AdapterDisconnected,
            "timeout" => Self::Timeout,
            "media_failure" => Self::MediaFailure,
            "forced_termination" => Self::ForcedTermination,
            "jupyterlab_kernel_restarted" => Self::JupyterLabKernelRestarted,
            _ => Self::CommandFailure,
        }
    }
}

impl From<CancelReason> for PlaybackInterruptionReason {
    fn from(reason: CancelReason) -> Self {
        match reason {
            CancelReason::UserStop => Self::UserStop,
            CancelReason::ClientExit => Self::ClientExit,
            CancelReason::CourseSwitch => Self::CourseSwitch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackResume {
    pub unit_path: String,
    pub step_id: String,
    pub step_started: bool,
    pub action_index: Option<usize>,
    pub recovery: Option<PlaybackRecovery>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackRecovery {
    pub reason: PlaybackInterruptionReason,
    pub command: String,
    pub output: Option<String>,
    pub output_truncated: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PlaybackState {
    #[default]
    Idle,
    Ready {
        position: Option<PlaybackPosition>,
    },
    Playing {
        action_id: String,
        action_label: String,
        position: PlaybackPosition,
    },
    Narrating {
        clip: NarrationClipRef,
        position: PlaybackPosition,
    },
    Paused {
        position: PlaybackPosition,
        interruption: PlaybackInterruption,
    },
    StepComplete {
        position: PlaybackPosition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        continuation: Option<NarrationClipContinuation>,
        requires_human: bool,
    },
    UnitComplete {
        position: PlaybackPosition,
    },
    Error {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<PlaybackPosition>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interruption: Option<PlaybackInterruption>,
    },
}

/// Terminal session used by environment checks. It must not alias a playback
/// session, so a successful check closes only the terminal it created.
pub const ENVIRONMENT_CHECK_SESSION_KEY: &str = "environment";

#[derive(Debug)]
pub enum PlaybackCommand {
    SetSkipUnitFinalWait(bool),
    SetSkipWaits(bool),
    SetCorrelation {
        correlation_id: String,
    },
    Play,
    Cancel {
        reason: CancelReason,
    },
    CancelPendingOneOff {
        reason: CancelReason,
    },
    WorkspaceReset {
        paths: Vec<String>,
        exclude_paths: Vec<String>,
        session_keys: Vec<String>,
        close_tabs: bool,
        completion: oneshot::Sender<Result<(), String>>,
    },
    ExecuteAction {
        action: Action,
        unit_path: String,
        completion: oneshot::Sender<Result<AckPayload, String>>,
    },
    ExecuteCheck {
        check: CheckPayload,
        completion: oneshot::Sender<Result<AckPayload, String>>,
    },
    ReplacePlan {
        steps: Vec<PlaybackStep>,
        resume: Option<PlaybackResume>,
        completion: oneshot::Sender<Result<(), String>>,
    },
    NarrationStarted {
        clip: NarrationClipRef,
    },
    NarrationEnded {
        clip: NarrationClipRef,
    },
    NarrationFailed {
        clip: NarrationClipRef,
        message: String,
    },
    PersistenceFailed {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackUnavailable;

impl std::fmt::Display for PlaybackUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "playback server is not available")
    }
}

impl std::error::Error for PlaybackUnavailable {}

#[derive(Clone)]
pub struct PlaybackCommandSender {
    pub(super) commands: mpsc::UnboundedSender<PlaybackCommand>,
}

impl PlaybackCommandSender {
    pub fn set_skip_unit_final_wait(&self, skip: bool) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::SetSkipUnitFinalWait(skip))
            .map_err(|_| PlaybackUnavailable)
    }

    /// Skips every `Wait` delay until disabled. Intended for deterministic
    /// verification runs that only need the action structure, not its timing.
    pub fn set_skip_waits(&self, skip: bool) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::SetSkipWaits(skip))
            .map_err(|_| PlaybackUnavailable)
    }

    /// Fire-and-forget: a live pending one-off or workspace reset drops the
    /// command.
    pub fn play(&self) -> Result<(), PlaybackUnavailable> {
        self.play_correlated(&Uuid::new_v4().to_string())
    }

    pub fn play_correlated(&self, correlation_id: &str) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::SetCorrelation {
                correlation_id: correlation_id.to_string(),
            })
            .map_err(|_| PlaybackUnavailable)?;
        self.commands
            .send(PlaybackCommand::Play)
            .map_err(|_| PlaybackUnavailable)
    }

    pub fn cancel(&self) -> Result<(), PlaybackUnavailable> {
        self.cancel_with_reason(CancelReason::UserStop)
    }

    pub fn cancel_with_reason(&self, reason: CancelReason) -> Result<(), PlaybackUnavailable> {
        self.cancel_correlated(reason, &Uuid::new_v4().to_string())
    }

    /// Cancels the pending one-off action/check without disturbing playback.
    pub fn cancel_pending_one_off(&self, reason: CancelReason) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::CancelPendingOneOff { reason })
            .map_err(|_| PlaybackUnavailable)
    }

    pub fn cancel_correlated(
        &self,
        reason: CancelReason,
        correlation_id: &str,
    ) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::SetCorrelation {
                correlation_id: correlation_id.to_string(),
            })
            .map_err(|_| PlaybackUnavailable)?;
        self.commands
            .send(PlaybackCommand::Cancel { reason })
            .map_err(|_| PlaybackUnavailable)
    }

    pub fn workspace_reset(
        &self,
        paths: Vec<String>,
    ) -> Result<oneshot::Receiver<Result<(), String>>, PlaybackUnavailable> {
        self.workspace_reset_correlated(paths, Vec::new(), &Uuid::new_v4().to_string())
    }

    pub fn workspace_reset_correlated(
        &self,
        paths: Vec<String>,
        exclude_paths: Vec<String>,
        correlation_id: &str,
    ) -> Result<oneshot::Receiver<Result<(), String>>, PlaybackUnavailable> {
        self.workspace_cleanup_correlated(paths, exclude_paths, Vec::new(), correlation_id, true)
    }

    pub fn terminal_cleanup(
        &self,
        paths: Vec<String>,
    ) -> Result<oneshot::Receiver<Result<(), String>>, PlaybackUnavailable> {
        self.terminal_cleanup_correlated(paths, Vec::new(), &Uuid::new_v4().to_string())
    }

    pub fn terminal_cleanup_correlated(
        &self,
        paths: Vec<String>,
        exclude_paths: Vec<String>,
        correlation_id: &str,
    ) -> Result<oneshot::Receiver<Result<(), String>>, PlaybackUnavailable> {
        self.workspace_cleanup_correlated(paths, exclude_paths, Vec::new(), correlation_id, false)
    }

    /// Close specific managed terminal sessions, independent of their cwd.
    pub fn terminal_cleanup_sessions(
        &self,
        session_keys: Vec<String>,
    ) -> Result<oneshot::Receiver<Result<(), String>>, PlaybackUnavailable> {
        self.workspace_cleanup_correlated(
            Vec::new(),
            Vec::new(),
            session_keys,
            &Uuid::new_v4().to_string(),
            false,
        )
    }

    fn workspace_cleanup_correlated(
        &self,
        paths: Vec<String>,
        exclude_paths: Vec<String>,
        session_keys: Vec<String>,
        correlation_id: &str,
        close_tabs: bool,
    ) -> Result<oneshot::Receiver<Result<(), String>>, PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::SetCorrelation {
                correlation_id: correlation_id.to_string(),
            })
            .map_err(|_| PlaybackUnavailable)?;
        let (completion, receiver) = oneshot::channel();
        self.commands
            .send(PlaybackCommand::WorkspaceReset {
                paths,
                exclude_paths,
                session_keys,
                close_tabs,
                completion,
            })
            .map_err(|_| PlaybackUnavailable)?;
        Ok(receiver)
    }

    pub fn execute_action(
        &self,
        action: Action,
        unit_path: &str,
    ) -> Result<oneshot::Receiver<Result<AckPayload, String>>, PlaybackUnavailable> {
        let (completion, receiver) = oneshot::channel();
        self.commands
            .send(PlaybackCommand::ExecuteAction {
                action,
                unit_path: unit_path.to_string(),
                completion,
            })
            .map_err(|_| PlaybackUnavailable)?;
        Ok(receiver)
    }

    pub fn execute_check(
        &self,
        check: CheckPayload,
    ) -> Result<oneshot::Receiver<Result<AckPayload, String>>, PlaybackUnavailable> {
        let (completion, receiver) = oneshot::channel();
        self.commands
            .send(PlaybackCommand::ExecuteCheck { check, completion })
            .map_err(|_| PlaybackUnavailable)?;
        Ok(receiver)
    }

    pub fn narration_ended(&self, clip: NarrationClipRef) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::NarrationEnded { clip })
            .map_err(|_| PlaybackUnavailable)
    }

    pub fn narration_started(&self, clip: NarrationClipRef) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::NarrationStarted { clip })
            .map_err(|_| PlaybackUnavailable)
    }

    pub fn narration_failed(
        &self,
        clip: NarrationClipRef,
        message: impl Into<String>,
    ) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::NarrationFailed {
                clip,
                message: message.into(),
            })
            .map_err(|_| PlaybackUnavailable)
    }

    pub fn persistence_failed(
        &self,
        message: impl Into<String>,
    ) -> Result<(), PlaybackUnavailable> {
        self.commands
            .send(PlaybackCommand::PersistenceFailed {
                message: message.into(),
            })
            .map_err(|_| PlaybackUnavailable)
    }

    pub fn replace_plan(
        &self,
        steps: Vec<PlaybackStep>,
        resume: Option<PlaybackResume>,
    ) -> Result<oneshot::Receiver<Result<(), String>>, PlaybackUnavailable> {
        let (completion, receiver) = oneshot::channel();
        self.commands
            .send(PlaybackCommand::ReplacePlan {
                steps,
                resume,
                completion,
            })
            .map_err(|_| PlaybackUnavailable)?;
        Ok(receiver)
    }
}

#[derive(Debug)]
pub enum ServerError {
    Io(std::io::Error),
    Discovery(DiscoveryError),
    PortInUse { port: u16 },
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "server I/O error: {error}"),
            Self::Discovery(error) => write!(formatter, "server discovery error: {error}"),
            Self::PortInUse { port } => {
                write!(
                    formatter,
                    "{}",
                    cynapse_client_i18n::t_with(
                        "error.jupyterlab_control_port_in_use",
                        &[("port", &port.to_string())],
                    )
                )
            }
        }
    }
}

impl std::error::Error for ServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Discovery(error) => Some(error),
            Self::PortInUse { .. } => None,
        }
    }
}

impl From<std::io::Error> for ServerError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<DiscoveryError> for ServerError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}
