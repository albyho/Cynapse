use crate::{PlaybackInterruptionReason, PlaybackState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LearningMode {
    ByStep,
    Continuous,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LearningPhase {
    #[default]
    Idle,
    Running,
    WaitingForStep,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LearningView {
    pub mode: Option<LearningMode>,
    pub phase: LearningPhase,
    pub unit_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LearningControllerError {
    AlreadyActive,
}

impl std::fmt::Display for LearningControllerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive => write!(formatter, "a learning mode is already active"),
        }
    }
}

impl std::error::Error for LearningControllerError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveLearning {
    mode: LearningMode,
    unit_path: String,
}

pub struct LearningController {
    active: Option<ActiveLearning>,
    phase: LearningPhase,
    error: Option<String>,
}

impl Default for LearningController {
    fn default() -> Self {
        Self {
            active: None,
            phase: LearningPhase::Idle,
            error: None,
        }
    }
}

impl LearningController {
    pub fn start(
        &mut self,
        mode: LearningMode,
        unit_path: &str,
    ) -> Result<LearningView, LearningControllerError> {
        if self.active.is_some() {
            return Err(LearningControllerError::AlreadyActive);
        }
        self.active = Some(ActiveLearning {
            mode,
            unit_path: unit_path.to_string(),
        });
        self.phase = LearningPhase::Running;
        self.error = None;
        Ok(self.view())
    }

    pub fn observe(&mut self, playback: &PlaybackState) -> LearningView {
        self.observe_with_adapter_loss(playback, true)
    }

    /// Observe playback with the caller's Unit-specific Adapter-loss decision.
    /// Local-only Units pass false, so a disconnect does not stop learning.
    pub fn observe_with_adapter_loss(
        &mut self,
        playback: &PlaybackState,
        adapter_loss: bool,
    ) -> LearningView {
        let Some(active) = self.active.as_ref() else {
            return self.view();
        };
        let continuous = active.mode == LearningMode::Continuous;
        // Keep this match exhaustive: a new PlaybackState variant must choose a
        // learning phase instead of silently falling through to Running.
        match playback {
            PlaybackState::StepComplete { requires_human, .. } => {
                self.phase = if *requires_human || !continuous {
                    LearningPhase::WaitingForStep
                } else {
                    LearningPhase::Running
                };
            }
            PlaybackState::UnitComplete { .. } => {
                if continuous {
                    self.phase = LearningPhase::Running;
                } else {
                    self.active = None;
                    self.phase = LearningPhase::Completed;
                }
            }
            PlaybackState::Error { message, .. } => {
                self.phase = LearningPhase::Failed;
                self.error = Some(message.clone());
            }
            PlaybackState::Paused { interruption, .. }
                if adapter_loss
                    && interruption.reason == PlaybackInterruptionReason::AdapterDisconnected =>
            {
                return self.stop();
            }
            PlaybackState::Idle => {}
            PlaybackState::Ready { .. }
            | PlaybackState::Playing { .. }
            | PlaybackState::Narrating { .. }
            | PlaybackState::Paused { .. } => {
                self.phase = LearningPhase::Running;
                self.error = None;
            }
        }
        self.view()
    }

    pub fn complete_unit(&mut self) -> LearningView {
        self.active = None;
        self.phase = LearningPhase::Completed;
        self.error = None;
        self.view()
    }

    pub fn move_to_unit(&mut self, unit_path: &str) -> LearningView {
        if let Some(active) = self.active.as_mut() {
            active.unit_path = unit_path.to_string();
            self.phase = LearningPhase::Running;
            self.error = None;
        }
        self.view()
    }

    pub fn fail(&mut self, message: impl Into<String>) -> LearningView {
        if self.active.is_some() {
            self.phase = LearningPhase::Failed;
            self.error = Some(message.into());
        }
        self.view()
    }

    pub fn stop(&mut self) -> LearningView {
        self.active = None;
        self.phase = LearningPhase::Idle;
        self.error = None;
        self.view()
    }

    pub fn view(&self) -> LearningView {
        LearningView {
            mode: self.active.as_ref().map(|active| active.mode),
            phase: self.phase,
            unit_path: self.active.as_ref().map(|active| active.unit_path.clone()),
            error: self.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
