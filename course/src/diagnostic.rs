//! Structured authoring diagnostics.
//!
//! `verify:content` and the Client loader gate must agree on whether a package
//! is valid. They may disagree on presentation, so the shared crate emits one
//! machine-readable finding instead of prose that each consumer re-parses.

use serde::{Deserialize, Serialize};

use super::CourseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// One finding about one place in a Course Package.
///
/// `node_path` is a Node Path (ids, not dir names) so a consumer can jump to
/// the node that owns the problem. `json_pointer` points inside the JSON
/// artifact named by `step_file` when the finding is at field level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub node_path: Option<String>,
    pub step_file: Option<String>,
    pub json_pointer: Option<String>,
    pub message: String,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            node_path: None,
            step_file: None,
            json_pointer: None,
            message: message.into(),
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, message)
        }
    }

    pub fn at_node(mut self, node_path: impl Into<String>) -> Self {
        self.node_path = Some(node_path.into());
        self
    }

    pub fn at_step_file(mut self, step_file: impl Into<String>) -> Self {
        self.step_file = Some(step_file.into());
        self
    }

    pub fn at_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.json_pointer = Some(pointer.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl From<&CourseError> for Diagnostic {
    fn from(error: &CourseError) -> Self {
        let code = match error {
            CourseError::Io(_) => "course.io",
            CourseError::Json(_) => "course.json",
            CourseError::Package(_) => "course.package",
        };
        Self::error(code, error.to_string())
    }
}

impl From<CourseError> for Diagnostic {
    fn from(error: CourseError) -> Self {
        Self::from(&error)
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}[{}]: {}",
            self.severity.as_str(),
            self.code,
            self.message
        )?;
        if let Some(node_path) = &self.node_path {
            write!(formatter, " (node `{node_path}`)")?;
        }
        if let Some(step_file) = &self.step_file {
            write!(formatter, " ({step_file})")?;
        }
        if let Some(pointer) = &self.json_pointer {
            write!(formatter, " at {pointer}")?;
        }
        Ok(())
    }
}
