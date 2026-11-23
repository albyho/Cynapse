//! Application layer for Cynapse Client.
//!
//! This crate owns the client-facing state projection, the core session/server
//! owner, and the use cases that operate on `cynapse-client-core`. It has no
//! GPUI, window, audio-device, or platform dependency: the GPUI app observes
//! this model and performs OS effects.
//!
//! Course projections are cached; mutate course state only through `apply` or
//! `apply_course_state` so `course_revision` invalidates the cache.

mod course_projection;
mod session;

use course_projection::CourseProjection;
pub use course_projection::{ProjectedCourseRow, UnitProgressKind, UnitRow, unit_progress_kind};

use cynapse_client_core::{
    ChallengeStatus, ChallengeView, ClientStateReducer, ClientTransitionKind, CourseSessionState,
    CourseTreeNodeView, LearningController, LearningMode, LearningPhase, LearningView,
    PendingLearningIntent, PlaybackInterruptionReason, PlaybackPosition, PlaybackState, StepView,
    UnitStatus, Warning, adapter_readiness, playback_is_retryable,
};
use cynapse_client_i18n::t_with;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

pub use cynapse_client_core::{ClientEvent, ClientSettings, ConnectionState, ThemePreference};
pub use session::SessionService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextTarget {
    Step,
    Unit(NextUnitTarget),
}

/// A Unit target plus the progress policy that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextUnitTarget {
    pub path: String,
    /// Unit whose completion or explicit cursor selected `path`.
    pub source: String,
    pub origin: NextUnitOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextUnitOrigin {
    /// Playback reached `UnitComplete` at `source`.
    Boundary,
    /// `source` is the explicitly selected completed Unit.
    Cursor,
}

/// What traversal does after a Unit completes. A studied next Unit is never
/// selected or reset without an explicit prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextUnitDecision<'a> {
    NoNextUnit,
    Select(&'a UnitRow),
    PromptReset(&'a UnitRow),
    PromptContinueOrReset(&'a UnitRow),
}

/// The Unit that follows `current_unit_path` in Course Tree order.
pub fn next_unit_after<'a>(units: &'a [UnitRow], current_unit_path: &str) -> Option<&'a UnitRow> {
    let current = units
        .iter()
        .position(|unit| unit.path == current_unit_path)?;
    units.get(current + 1)
}

pub fn next_unit_decision<'a>(
    units: &'a [UnitRow],
    completed_unit_path: &str,
) -> NextUnitDecision<'a> {
    let Some(next) = next_unit_after(units, completed_unit_path) else {
        return NextUnitDecision::NoNextUnit;
    };
    match unit_progress_kind(next) {
        UnitProgressKind::New => NextUnitDecision::Select(next),
        UnitProgressKind::Completed => NextUnitDecision::PromptReset(next),
        UnitProgressKind::Partial => NextUnitDecision::PromptContinueOrReset(next),
    }
}

/// The next Unit that needs a learner decision before it is touched.
pub fn prompted_next_unit<'a>(
    units: &'a [UnitRow],
    current_unit_path: &str,
) -> Option<&'a UnitRow> {
    match next_unit_decision(units, current_unit_path) {
        NextUnitDecision::PromptReset(unit) | NextUnitDecision::PromptContinueOrReset(unit) => {
            Some(unit)
        }
        NextUnitDecision::NoNextUnit | NextUnitDecision::Select(_) => None,
    }
}

impl NextUnitTarget {
    /// The following Unit that needs a learner decision before it is touched.
    pub fn prompted_unit<'a>(&self, units: &'a [UnitRow]) -> Option<&'a UnitRow> {
        prompted_next_unit(units, &self.source)
    }
}

fn unit_completion_ends_learning(units: &[UnitRow], position: &PlaybackPosition) -> bool {
    matches!(
        next_unit_decision(units, &position.unit_path),
        NextUnitDecision::NoNextUnit
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeOutcome {
    Passed,
    Failed,
    Skipped,
    WorkspaceDirty,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeCheckResult {
    pub outcome: ChallengeOutcome,
    pub output: Option<String>,
    pub message: Option<String>,
    pub exit_code: Option<i32>,
    pub output_truncated: bool,
}

impl ChallengeCheckResult {
    pub fn new(outcome: ChallengeOutcome) -> Self {
        Self {
            outcome,
            output: None,
            message: None,
            exit_code: None,
            output_truncated: false,
        }
    }
}

pub struct ClientModel {
    settings: ClientSettings,
    state: ClientStateReducer,
    session: SessionService,
    learning: LearningController,
    learning_generation: u64,
    course_revision: u64,
    course_projection: CourseProjection,
}

impl ClientModel {
    pub fn new(settings: ClientSettings) -> Result<(Self, UnboundedReceiver<ClientEvent>), String> {
        Self::new_with_startup_course(settings, None)
    }

    /// Starts with an explicit Course Package path, or restores the persisted
    /// Active Course when the path is absent or cannot be opened.
    pub fn new_with_startup_course(
        settings: ClientSettings,
        startup_course: Option<std::path::PathBuf>,
    ) -> Result<(Self, UnboundedReceiver<ClientEvent>), String> {
        let (events, receiver) = tokio::sync::mpsc::unbounded_channel();
        let session = SessionService::new(events)?;
        Self::from_session(settings, session, receiver, startup_course)
    }

    /// Test seam: use an isolated state database and runtime discovery file.
    #[doc(hidden)]
    pub fn new_at(
        settings: ClientSettings,
        db_path: std::path::PathBuf,
    ) -> Result<(Self, UnboundedReceiver<ClientEvent>), String> {
        Self::new_at_with_startup_course(settings, db_path, None)
    }

    /// Test seam: isolated state database plus an explicit startup course.
    #[doc(hidden)]
    pub fn new_at_with_startup_course(
        settings: ClientSettings,
        db_path: std::path::PathBuf,
        startup_course: Option<std::path::PathBuf>,
    ) -> Result<(Self, UnboundedReceiver<ClientEvent>), String> {
        let (events, receiver) = tokio::sync::mpsc::unbounded_channel();
        let session = SessionService::new_at(events, db_path)?;
        Self::from_session(settings, session, receiver, startup_course)
    }

    fn from_session(
        settings: ClientSettings,
        session: SessionService,
        receiver: UnboundedReceiver<ClientEvent>,
        startup_course: Option<std::path::PathBuf>,
    ) -> Result<(Self, UnboundedReceiver<ClientEvent>), String> {
        let startup_error =
            startup_course.and_then(|path| session.open_course_at_startup(path).err());
        let initial = session.course_state()?;
        let mut state = ClientStateReducer::default();
        state.apply(&[ClientEvent::Course(Arc::new(initial))]);
        let mut warnings = Vec::new();
        if let Some(error) = startup_error {
            warnings
                .push(t_with("warning.startup_course_open_failed", &[("error", &error)]).into());
        }
        if let Err(error) = session.start_server_at_startup() {
            warnings.push(error.into());
        }
        if !warnings.is_empty() {
            state.apply(&[ClientEvent::Warnings(warnings)]);
        }
        let mut model = Self {
            settings,
            state,
            session,
            learning: LearningController::default(),
            learning_generation: 0,
            course_revision: 0,
            course_projection: CourseProjection::default(),
        };
        model.refresh_course_cache();
        Ok((model, receiver))
    }

    /// Applies one typed fact and reports whether the visible state changed.
    pub fn apply(&mut self, event: ClientEvent) -> bool {
        let course_changed = matches!(&event, ClientEvent::Course(_));
        let mut updates = Vec::new();
        match event {
            ClientEvent::Playback(playback) => {
                let had_mode = self.state.learning().mode.is_some();
                let adapter_lost = self.playback_marks_adapter_loss(&playback);
                let final_unit_complete = matches!(
                    playback.as_ref(),
                    PlaybackState::UnitComplete { position }
                        if unit_completion_ends_learning(self.units(), position)
                );
                let learning = learning_after_playback(
                    &mut self.learning,
                    &playback,
                    final_unit_complete,
                    adapter_lost,
                );
                if had_mode && learning.mode.is_none() && learning.phase == LearningPhase::Idle {
                    updates.push(ClientEvent::PendingLearningIntent(None));
                }
                updates.push(ClientEvent::Playback(playback));
                updates.push(ClientEvent::Learning(learning));
                if adapter_lost {
                    self.learning_generation = self.learning_generation.wrapping_add(1);
                    self.push_adapter_disconnected_warning(&mut updates);
                }
            }
            ClientEvent::Connection(connection) => {
                let adapter_lost = self.adapter_loss_ends_learning(&connection);
                updates.push(ClientEvent::Connection(connection));
                if adapter_lost {
                    self.learning_generation = self.learning_generation.wrapping_add(1);
                    updates.push(ClientEvent::Learning(self.learning.stop()));
                    updates.push(ClientEvent::PendingLearningIntent(None));
                    self.push_adapter_disconnected_warning(&mut updates);
                }
            }
            // Every remaining fact reaches the reducer unchanged; only Playback
            // and Connection can produce more than one.
            other => updates.push(other),
        }
        let changed = self.state.apply(&updates);
        if course_changed {
            self.course_revision = self.course_revision.wrapping_add(1);
            self.refresh_course_cache();
        }
        changed
    }

    pub fn apply_course_state(&mut self, state: CourseSessionState) {
        self.state.apply(&[ClientEvent::Course(Arc::new(state))]);
        self.course_revision = self.course_revision.wrapping_add(1);
        self.refresh_course_cache();
    }

    fn refresh_course_cache(&mut self) {
        self.course_projection.refresh(self.state.course());
    }

    pub fn course_revision(&self) -> u64 {
        self.course_revision
    }

    pub fn course_content_revision(&self) -> u64 {
        self.course_projection.content_revision()
    }

    pub fn set_warning(&mut self, warning: impl Into<Warning>) {
        self.state
            .apply(&[ClientEvent::Warnings(vec![warning.into()])]);
    }

    pub fn clear_warnings(&mut self) {
        self.state.apply(&[ClientEvent::Warnings(Vec::new())]);
    }

    pub fn start_learning(&mut self, mode: LearningMode, unit_path: &str) -> Result<(), String> {
        let adapters = self
            .units()
            .iter()
            .find(|unit| unit.path == unit_path)
            .map(|unit| unit.adapters.as_slice())
            .unwrap_or(&[]);
        // A JupyterLab start opens the browser first; SessionService waits for
        // the extension before requiring readiness.
        if !adapters.contains(&cynapse_client_core::AdapterId::Jupyterlab) {
            adapter_readiness(adapters, self.state.connection())
                .map_err(|warning| warning.text())?;
        }
        let view = self
            .learning
            .start(mode, unit_path)
            .map_err(|error| error.to_string())?;
        self.learning_generation = self.learning_generation.wrapping_add(1);
        self.state.apply(&[ClientEvent::Learning(view)]);
        Ok(())
    }

    /// Starts the local learning view when playback was already advanced by
    /// `SessionService`; unlike `start_learning`, this does not begin a second
    /// session transition.
    pub fn start_learning_local(
        &mut self,
        mode: LearningMode,
        unit_path: &str,
    ) -> Result<(), String> {
        let view = self
            .learning
            .start(mode, unit_path)
            .map_err(|error| error.to_string())?;
        self.learning_generation = self.learning_generation.wrapping_add(1);
        self.state.apply(&[ClientEvent::Learning(view)]);
        Ok(())
    }

    pub fn fail_learning(&mut self, message: String) {
        let view = self.learning.fail(message);
        self.state.apply(&[ClientEvent::Learning(view)]);
    }

    pub fn stop_learning_local(&mut self) {
        self.learning_generation = self.learning_generation.wrapping_add(1);
        let view = self.learning.stop();
        self.state.apply(&[ClientEvent::Learning(view)]);
    }

    /// Stops the provisional mode of a start attempt only if no newer start or
    /// stop superseded it.
    pub fn stop_learning_after_start_failure(&mut self, generation: u64) -> bool {
        if self.learning_generation != generation {
            return false;
        }
        self.stop_learning_local();
        true
    }

    pub fn learning_generation(&self) -> u64 {
        self.learning_generation
    }

    pub fn pending_learning_intent(&self) -> Option<&PendingLearningIntent> {
        self.state.pending_learning_intent()
    }

    pub fn set_pending_learning_intent(&mut self, intent: Option<PendingLearningIntent>) {
        self.state
            .apply(&[ClientEvent::PendingLearningIntent(intent)]);
    }

    pub fn move_learning_unit(&mut self, unit_path: &str) {
        let view = self.learning.move_to_unit(unit_path);
        self.state.apply(&[ClientEvent::Learning(view)]);
    }

    pub fn settings(&self) -> &ClientSettings {
        &self.settings
    }

    pub fn state(&self) -> &ClientStateReducer {
        &self.state
    }

    pub fn session(&self) -> SessionService {
        self.session.clone()
    }

    pub fn connection(&self) -> &ConnectionState {
        self.state.connection()
    }

    pub fn course_title(&self) -> Option<&str> {
        self.state
            .course()
            .active_course
            .as_ref()
            .map(|course| course.title.as_str())
    }

    pub fn course_identity(&self) -> Option<(String, String)> {
        let course = self.state.course().active_course.as_ref()?;
        Some((
            course.id.clone(),
            self.state
                .course()
                .active_course_path
                .clone()
                .unwrap_or_default(),
        ))
    }

    /// Whether any Unit has started or completed. Selection and the resume
    /// cursor are not progress.
    pub fn has_progress(&self) -> bool {
        self.course_projection.has_progress()
    }

    pub fn recent_courses(&self) -> &[cynapse_client_core::RecentCourse] {
        &self.state.course().recent_courses
    }

    /// Explicit learning/UI selection. Never derived from the resume cursor.
    pub fn selected_unit_path(&self) -> Option<&str> {
        self.state
            .course()
            .selected_unit
            .as_ref()
            .map(|unit| unit.path.as_str())
    }

    pub fn is_learning(&self) -> bool {
        matches!(
            self.state.learning().phase,
            LearningPhase::Running | LearningPhase::WaitingForStep | LearningPhase::Failed
        )
    }

    pub fn current_position(&self) -> Option<&PlaybackPosition> {
        match self.state.playback() {
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

    pub fn current_step(&self) -> Option<&StepView> {
        let position = self.current_position()?;
        let course = self.state.course().active_course.as_ref()?;
        let unit = cynapse_client_core::find_unit_by_path(&course.tree, &position.unit_path)?;
        unit.steps
            .iter()
            .find(|step| step.step_id == position.step_id)
    }

    pub fn current_challenge(&self) -> Option<&ChallengeView> {
        self.current_step()?.challenge.as_ref()
    }

    pub fn current_challenge_status(&self) -> Option<ChallengeStatus> {
        step_challenge_status(self.current_step())
    }

    pub fn current_challenge_attempts(&self) -> u32 {
        self.current_step()
            .map(|step| step.challenge_attempts)
            .unwrap_or(0)
    }

    pub fn can_advance_step(&self) -> bool {
        if !matches!(self.state.playback(), PlaybackState::StepComplete { .. }) {
            return false;
        }
        match self.current_challenge_status() {
            Some(ChallengeStatus::Passed | ChallengeStatus::Skipped) | None => true,
            Some(ChallengeStatus::NotStarted | ChallengeStatus::Failed) => false,
        }
    }

    pub fn at_checkpoint(&self) -> bool {
        matches!(self.state.playback(), PlaybackState::StepComplete { .. })
            && self.current_position().is_some_and(|position| {
                checkpoint_matches_selection(position, self.selected_unit_path())
            })
    }

    pub fn at_step_boundary(&self) -> bool {
        self.at_checkpoint() || self.at_final_challenge_checkpoint()
    }

    pub fn at_final_challenge_checkpoint(&self) -> bool {
        let PlaybackState::StepComplete {
            position,
            continuation: None,
            ..
        } = self.state.playback()
        else {
            return false;
        };
        self.current_step()
            .is_some_and(|step| step.challenge.is_some())
            && final_challenge_advances_unit(
                self.is_learning(),
                position.unit_step_index + 1 >= position.unit_step_total,
                matches!(
                    self.current_challenge_status(),
                    Some(ChallengeStatus::Passed | ChallengeStatus::Skipped)
                ),
            )
    }

    pub fn at_unit_boundary(&self) -> bool {
        matches!(self.state.playback(), PlaybackState::UnitComplete { .. })
            && self.current_position().is_some_and(|position| {
                self.selected_unit_path()
                    .is_none_or(|selected| selected == position.unit_path)
            })
    }

    pub fn at_completed_unit_cursor(&self) -> bool {
        if matches!(
            self.state.playback(),
            PlaybackState::Playing { .. } | PlaybackState::Narrating { .. }
        ) {
            return false;
        }
        let Some(selected) = self.selected_unit_path() else {
            return false;
        };
        self.units()
            .iter()
            .any(|unit| unit.path == selected && unit.status == UnitStatus::Completed)
    }

    pub fn next_target(&self) -> Option<NextTarget> {
        if matches!(
            self.state.playback(),
            PlaybackState::StepComplete {
                continuation: None,
                ..
            }
        ) && self.at_step_boundary()
            && self.can_advance_step()
        {
            return Some(NextTarget::Step);
        }
        if self.at_unit_boundary()
            && let PlaybackState::UnitComplete { position } = self.state.playback()
            && let Some(next) = next_unit_after(self.units(), &position.unit_path)
        {
            return Some(NextTarget::Unit(NextUnitTarget {
                path: next.path.clone(),
                source: position.unit_path.clone(),
                origin: NextUnitOrigin::Boundary,
            }));
        }
        if self.at_completed_unit_cursor()
            && let Some(selected) = self.selected_unit_path()
            && let Some(next) = next_unit_after(self.units(), selected)
        {
            return Some(NextTarget::Unit(NextUnitTarget {
                path: next.path.clone(),
                source: selected.to_string(),
                origin: NextUnitOrigin::Cursor,
            }));
        }
        None
    }

    pub fn next_unit(&self) -> Option<&UnitRow> {
        let current = self
            .current_position()
            .map(|position| position.unit_path.as_str())
            .or_else(|| self.selected_unit_path())?;
        next_unit_after(self.units(), current)
    }

    pub fn can_retry(&self) -> bool {
        self.state.learning().mode.is_some() && playback_is_retryable(self.state.playback())
    }

    /// Whether this Playback state is the Adapter-disconnect pause that must
    /// end the active Learning Mode. The server may publish it before the
    /// Connection watch reports Waiting, so the warning cannot rely on the
    /// later Connection event.
    pub fn playback_marks_adapter_loss(&self, playback: &PlaybackState) -> bool {
        if self
            .state
            .transition()
            .is_some_and(|transition| transition.kind == ClientTransitionKind::Starting)
            || self.state.learning().mode.is_none()
        {
            return false;
        }
        let Some(adapters) = self.active_unit_adapters() else {
            return false;
        };
        !adapters.is_empty()
            && matches!(
                playback,
                PlaybackState::Paused { interruption, .. }
                    if interruption.reason
                        == PlaybackInterruptionReason::AdapterDisconnected
            )
    }

    fn push_adapter_disconnected_warning(&self, updates: &mut Vec<ClientEvent>) {
        let mut warnings = self.state.warnings().to_vec();
        if !warnings.iter().any(Warning::is_adapter_disconnected) {
            warnings.push(Warning::AdapterDisconnected);
        }
        updates.push(ClientEvent::Warnings(warnings));
    }

    /// Whether this connection state ends the active Learning Mode because a
    /// required Adapter became unavailable. Units without required Adapters are
    /// unaffected.
    pub fn adapter_loss_ends_learning(&self, connection: &ConnectionState) -> bool {
        // A Start attempt opens/reconnects the Adapter before playback begins.
        // Treating that transient socket drop as an Adapter loss would cancel
        // the Start and flash a misleading "not connected" warning.
        if self
            .state
            .transition()
            .is_some_and(|transition| transition.kind == ClientTransitionKind::Starting)
        {
            return false;
        }
        if self.state.learning().mode.is_none() {
            return false;
        }
        let Some(adapters) = self.active_unit_adapters() else {
            return false;
        };
        adapter_readiness(adapters, connection).is_err()
    }

    fn active_unit_adapters(&self) -> Option<&[cynapse_client_core::AdapterId]> {
        let unit_path = self.state.learning().unit_path.as_deref()?;
        Some(
            self.units()
                .iter()
                .find(|unit| unit.path == unit_path)
                .map(|unit| unit.adapters.as_slice())
                .unwrap_or(&[]),
        )
    }

    pub fn course_tree(&self) -> &[CourseTreeNodeView] {
        self.state
            .course()
            .active_course
            .as_ref()
            .map(|course| course.tree.as_slice())
            .unwrap_or_default()
    }

    pub fn units(&self) -> &[UnitRow] {
        self.course_projection.units()
    }

    pub fn course_rows(
        &self,
    ) -> std::sync::Arc<std::collections::HashMap<String, ProjectedCourseRow>> {
        self.course_projection.rows()
    }

    pub fn update_settings(&mut self, update: impl FnOnce(&mut ClientSettings)) {
        update(&mut self.settings);
        self.settings.caption = self.settings.caption.clone().clamped();
    }
}

fn learning_after_playback(
    learning: &mut LearningController,
    playback: &PlaybackState,
    final_unit_complete: bool,
    adapter_loss: bool,
) -> LearningView {
    let view = learning.observe_with_adapter_loss(playback, adapter_loss);
    if final_unit_complete && view.mode.is_some() {
        learning.complete_unit()
    } else {
        view
    }
}

fn checkpoint_matches_selection(
    position: &PlaybackPosition,
    selected_unit_path: Option<&str>,
) -> bool {
    selected_unit_path == Some(position.unit_path.as_str())
}

fn final_challenge_advances_unit(
    learning_active: bool,
    unit_last_step: bool,
    challenge_finished: bool,
) -> bool {
    learning_active && unit_last_step && challenge_finished
}

fn step_challenge_status(step: Option<&StepView>) -> Option<ChallengeStatus> {
    step.filter(|step| step.challenge.is_some())
        .map(|step| step.challenge_status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cynapse_client_core::{CourseView, UnitStatus, UnitView};

    #[test]
    fn a_presentation_step_has_no_challenge_status() {
        let step = StepView {
            step_id: "presentation".to_string(),
            title: "Presentation".to_string(),
            challenge: None,
            challenge_status: ChallengeStatus::NotStarted,
            challenge_attempts: 0,
            started: false,
            completed: false,
            requires_human: false,
        };

        assert_eq!(step_challenge_status(Some(&step)), None);
    }

    #[test]
    fn a_challenge_step_exposes_its_status() {
        let step = StepView {
            step_id: "challenge".to_string(),
            title: "Challenge".to_string(),
            challenge: Some(ChallengeView {
                instructions: "Do it".to_string(),
                hint: None,
                allow_skip: true,
            }),
            challenge_status: ChallengeStatus::NotStarted,
            challenge_attempts: 2,
            started: false,
            completed: false,
            requires_human: false,
        };

        assert_eq!(
            step_challenge_status(Some(&step)),
            Some(ChallengeStatus::NotStarted)
        );
    }

    #[test]
    fn final_unit_complete_ends_continuous_learning() {
        let mut learning = LearningController::default();
        learning
            .start(LearningMode::Continuous, "unit")
            .expect("start learning");
        let view = learning_after_playback(
            &mut learning,
            &PlaybackState::UnitComplete {
                position: PlaybackPosition::default(),
            },
            true,
            false,
        );
        assert_eq!(view.mode, None);
        assert_eq!(view.phase, cynapse_client_core::LearningPhase::Completed);
    }

    fn unit_row(path: &str, status: UnitStatus) -> UnitRow {
        UnitRow {
            path: path.to_string(),
            title: path.to_string(),
            status,
            adapters: Vec::new(),
        }
    }

    #[test]
    fn unit_completion_uses_the_traversal_seam_not_the_last_array_slot() {
        let units = vec![
            unit_row("branch/first", UnitStatus::Completed),
            unit_row("branch/second", UnitStatus::NotStarted),
        ];
        let first = PlaybackPosition {
            unit_path: "branch/first".to_string(),
            ..Default::default()
        };
        assert!(
            !unit_completion_ends_learning(&units, &first),
            "a Unit with a next Unit keeps traversal open"
        );

        let last = PlaybackPosition {
            unit_path: "branch/second".to_string(),
            ..Default::default()
        };
        assert!(unit_completion_ends_learning(&units, &last));

        let outside = PlaybackPosition {
            unit_path: "branch/outside".to_string(),
            ..Default::default()
        };
        assert!(
            unit_completion_ends_learning(&units, &outside),
            "a completed path with no traversal successor ends learning even though it is not the array-last Unit"
        );
    }

    #[test]
    fn a_nonfinal_unit_complete_keeps_continuous_learning_active() {
        let mut learning = LearningController::default();
        learning
            .start(LearningMode::Continuous, "unit")
            .expect("start learning");
        let view = learning_after_playback(
            &mut learning,
            &PlaybackState::UnitComplete {
                position: PlaybackPosition::default(),
            },
            false,
            false,
        );
        assert_eq!(view.mode, Some(LearningMode::Continuous));
        assert_eq!(view.phase, cynapse_client_core::LearningPhase::Running);
    }

    /// A one-Unit course whose second Step is a Challenge.
    fn course_with_a_challenge(status: ChallengeStatus) -> CourseSessionState {
        let steps = vec![
            StepView {
                step_id: "presentation".to_string(),
                title: "Presentation".to_string(),
                challenge: None,
                challenge_status: ChallengeStatus::NotStarted,
                challenge_attempts: 0,
                started: true,
                completed: true,
                requires_human: false,
            },
            StepView {
                step_id: "challenge".to_string(),
                title: "Challenge".to_string(),
                challenge: Some(ChallengeView {
                    instructions: "Do it".to_string(),
                    hint: None,
                    allow_skip: true,
                }),
                challenge_status: status,
                challenge_attempts: 1,
                started: true,
                completed: true,
                requires_human: false,
            },
        ];
        let unit = UnitView {
            path: "ch/unit".to_string(),
            title: "Unit".to_string(),
            steps,
            current_step_id: None,
            adapters: Vec::new(),
            status: UnitStatus::InProgress,
            completed: false,
            skipped_challenges: 0,
        };
        CourseSessionState {
            active_course: Some(CourseView {
                id: "course".to_string(),
                title: "Course".to_string(),
                version: "1".to_string(),
                adapters: Vec::new(),
                workspace_path: None,
                tree: vec![CourseTreeNodeView {
                    path: "ch/unit".to_string(),
                    title: "Unit".to_string(),
                    completed: false,
                    unit: Some(unit.clone()),
                    children: Vec::new(),
                }],
                current_unit_path: Some("ch/unit".to_string()),
                current_step_id: Some("challenge".to_string()),
                recovery: None,
            }),
            active_course_path: Some("course".to_string()),
            selected_unit: Some(unit),
            ..Default::default()
        }
    }

    fn challenge_step_complete(unit_step_index: usize) -> ClientEvent {
        ClientEvent::Playback(Box::new(PlaybackState::StepComplete {
            position: PlaybackPosition {
                unit_path: "ch/unit".to_string(),
                step_id: "challenge".to_string(),
                unit_step_index,
                unit_step_total: 2,
                ..Default::default()
            },
            continuation: None,
            requires_human: false,
        }))
    }

    fn model_with_challenge(status: ChallengeStatus) -> ClientModel {
        let directory = tempfile::tempdir().unwrap();
        let (mut model, _receiver) =
            ClientModel::new_at(ClientSettings::default(), directory.path().join("state.db"))
                .unwrap();
        model.apply_course_state(course_with_a_challenge(status));
        model
            .start_learning_local(LearningMode::ByStep, "ch/unit")
            .unwrap();
        model
    }

    /// The rule that keeps playback from waiting for a click after the last
    /// Challenge: its completion makes the Unit boundary the next stop.
    #[test]
    fn a_finished_final_challenge_is_the_unit_boundary() {
        let mut model = model_with_challenge(ChallengeStatus::Passed);

        model.apply(challenge_step_complete(1));
        assert!(model.at_final_challenge_checkpoint());

        model.apply(challenge_step_complete(0));
        assert!(
            !model.at_final_challenge_checkpoint(),
            "the same Challenge earlier in the Unit still waits for the learner"
        );
    }

    #[test]
    fn an_unfinished_final_challenge_is_not_the_unit_boundary() {
        let mut model = model_with_challenge(ChallengeStatus::NotStarted);

        model.apply(challenge_step_complete(1));
        assert!(!model.at_final_challenge_checkpoint());
    }

    #[test]
    fn a_stale_unit_complete_does_not_end_a_new_start() {
        let directory = tempfile::tempdir().unwrap();
        let (mut model, _receiver) =
            ClientModel::new_at(ClientSettings::default(), directory.path().join("state.db"))
                .unwrap();
        model.apply_course_state(course_with_a_challenge(ChallengeStatus::NotStarted));
        model.apply(ClientEvent::Playback(Box::new(
            PlaybackState::UnitComplete {
                position: PlaybackPosition {
                    unit_path: "ch/unit".to_string(),
                    ..Default::default()
                },
            },
        )));

        model
            .start_learning(LearningMode::ByStep, "ch/unit")
            .expect("start learning");

        assert!(model.is_learning());
        assert_eq!(
            model.state().learning().phase,
            cynapse_client_core::LearningPhase::Running
        );
    }
}
