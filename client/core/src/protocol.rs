use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Hello,
    HelloAck,
    Action,
    Check,
    Ack,
    Cancel,
    WorkspaceReset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope {
    pub v: u32,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: MessageType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_path: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloPayload {
    pub token: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloAckPayload {
    pub adapter: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AckStatus {
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AckPayload {
    pub status: AckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_marker_found: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timed_out: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckPayload {
    #[serde(rename = "type")]
    pub kind: String,
    pub unit_path: String,
    pub session_key: String,
    #[serde(default = "default_check_cwd")]
    pub cwd: String,
    pub command: String,
    #[serde(default = "default_check_exit_codes")]
    pub accepted_exit_codes: Vec<i32>,
    #[serde(default)]
    pub expected_output: Vec<String>,
    #[serde(default = "default_check_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub save_dirty: bool,
}

fn default_check_cwd() -> String {
    ".".to_string()
}

fn default_check_exit_codes() -> Vec<i32> {
    vec![0]
}

fn default_check_timeout_ms() -> u64 {
    300_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CancelPayload {
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<CancelReason>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    UserStop,
    ClientExit,
    CourseSwitch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceResetPayload {
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_paths: Vec<String>,
    /// When non-empty, only these managed terminal sessions are closed and
    /// `paths` is ignored. An environment check uses this to close the terminal
    /// it created without touching terminals other Units left open.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_keys: Vec<String>,
    #[serde(default = "default_true")]
    pub close_tabs: bool,
}

fn default_true() -> bool {
    true
}

impl Envelope {
    pub fn new(kind: MessageType, payload: serde_json::Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: Uuid::new_v4().to_string(),
            kind,
            unit_path: None,
            payload,
        }
    }

    pub fn hello(token: impl Into<String>) -> Self {
        Self::new(
            MessageType::Hello,
            serde_json::to_value(HelloPayload {
                token: token.into(),
                role: "adapter".to_string(),
            })
            .expect("hello payload is serializable"),
        )
    }

    pub fn hello_ack(adapter: impl Into<String>) -> Self {
        Self::new(
            MessageType::HelloAck,
            serde_json::to_value(HelloAckPayload {
                adapter: adapter.into(),
            })
            .expect("hello_ack payload is serializable"),
        )
    }

    pub fn check(payload: CheckPayload) -> Self {
        Self::new(
            MessageType::Check,
            serde_json::to_value(payload).expect("check payload is serializable"),
        )
    }

    pub fn cancel(target_id: impl Into<String>) -> Self {
        Self::cancel_with_reason(target_id, CancelReason::UserStop)
    }

    pub fn cancel_with_reason(target_id: impl Into<String>, reason: CancelReason) -> Self {
        Self::new(
            MessageType::Cancel,
            serde_json::to_value(CancelPayload {
                target_id: target_id.into(),
                reason: Some(reason),
            })
            .expect("cancel payload is serializable"),
        )
    }

    pub fn workspace_reset(paths: Vec<String>, exclude_paths: Vec<String>) -> Self {
        Self::new(
            MessageType::WorkspaceReset,
            serde_json::to_value(WorkspaceResetPayload {
                paths,
                exclude_paths,
                session_keys: Vec::new(),
                close_tabs: true,
            })
            .expect("workspace reset payload is serializable"),
        )
    }

    pub fn terminal_cleanup(paths: Vec<String>, exclude_paths: Vec<String>) -> Self {
        Self::new(
            MessageType::WorkspaceReset,
            serde_json::to_value(WorkspaceResetPayload {
                paths,
                exclude_paths,
                session_keys: Vec::new(),
                close_tabs: false,
            })
            .expect("terminal cleanup payload is serializable"),
        )
    }

    pub fn terminal_cleanup_sessions(session_keys: Vec<String>) -> Self {
        Self::new(
            MessageType::WorkspaceReset,
            serde_json::to_value(WorkspaceResetPayload {
                paths: Vec::new(),
                exclude_paths: Vec::new(),
                session_keys,
                close_tabs: false,
            })
            .expect("terminal cleanup payload is serializable"),
        )
    }

    pub fn ack_error(
        target_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: target_id.into(),
            kind: MessageType::Ack,
            unit_path: None,
            payload: serde_json::to_value(AckPayload {
                status: AckStatus::Error,
                code: Some(code.into()),
                message: Some(message.into()),
                exit_code: None,
                output: None,
                output_truncated: None,
                completion_marker_found: None,
                timed_out: None,
            })
            .expect("ack payload is serializable"),
        }
    }

    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
