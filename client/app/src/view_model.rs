//! GPUI client state derived from `ClientModel`.
//!
//! - selectors, labels, and the `NextAction` target/automatic/prompt decision
//! - playback, Challenge, Unit traversal, prompt, reset, and warning plumbing
//! - the single `can_run_next_action` execution gate used by the toolbar
//!
//! Pure decisions are tested in `view_model/tests.rs`; headless control-surface
//! tests live in `client/app/tests/ui.rs`.

use crate::audio::{NarrationEvent, NarrationService};
use cynapse_client_core::{
    AdapterId, CaptionFontFamily, CaptionPosition, ChallengeStatus, ChallengeView,
    ClientTransitionKind, ClientTransitionPhase, ClientTransitionStage, CourseSessionState,
    LanguagePreference, LearningMode, LearningPhase, NarrationClipRef, PendingLearningIntent,
    PlaybackInterruptionReason, PlaybackState, RecentCourse, UnitStatus, Warning,
    course_import::{CourseImportProgress, CourseImportStage},
};
use cynapse_client_i18n::{t, t_with};
use cynapse_client_model::{
    ChallengeCheckResult, ChallengeOutcome, ClientEvent, ClientModel, ClientSettings,
    ConnectionState, ProjectedCourseRow, SessionService, ThemePreference, UnitRow,
    prompted_next_unit,
};
pub(crate) use cynapse_client_model::{NextTarget, NextUnitOrigin};
use gpui_kit::{AppContext, Context, EventEmitter};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseImportView {
    pub label: String,
    pub steps: Vec<String>,
    pub active_step: Option<usize>,
    pub step_details: Vec<Option<String>>,
    pub cancellable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseImportErrorView {
    pub id: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseImportConflictView {
    pub id: u64,
    pub url: String,
    pub target: PathBuf,
}

struct ActiveCourseImport {
    view: CourseImportView,
    cancel: Arc<AtomicBool>,
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterStatus {
    pub tooltip: String,
    pub connected: bool,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NextAction {
    pub target: Option<NextTarget>,
    pub automatic: bool,
    pub prompt: bool,
}

impl NextAction {
    fn none() -> Self {
        Self {
            target: None,
            automatic: false,
            prompt: false,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, PartialEq, Eq)]
pub struct LearnerWaitKey {
    playback: PlaybackState,
    next_target: Option<NextTarget>,
    prompt_target: Option<String>,
    adapter_ready: bool,
}

/// Whether playback is waiting for the learner because the current Step
/// requires human input or is a Challenge.
fn awaiting_human_state(playback: &PlaybackState) -> bool {
    matches!(
        playback,
        PlaybackState::StepComplete {
            requires_human: true,
            ..
        }
    )
}

fn interruption_is_passive(reason: PlaybackInterruptionReason) -> bool {
    matches!(
        reason,
        PlaybackInterruptionReason::AdapterDisconnected
            | PlaybackInterruptionReason::Timeout
            | PlaybackInterruptionReason::CommandFailure
            | PlaybackInterruptionReason::MediaFailure
            | PlaybackInterruptionReason::ForcedTermination
            | PlaybackInterruptionReason::JupyterLabKernelRestarted
    )
}

fn has_passive_pause(playback: &PlaybackState) -> bool {
    matches!(
        playback,
        PlaybackState::Paused { interruption, .. }
            if interruption_is_passive(interruption.reason)
    )
}

fn passive_wait_needs_learner(adapter_ready: bool, playback: &PlaybackState) -> bool {
    matches!(playback, PlaybackState::Error { .. })
        || (!adapter_ready
            && (matches!(playback, PlaybackState::StepComplete { .. })
                || has_passive_pause(playback)))
}

fn learner_action_pending(
    advance_pending: bool,
    step_advance_pending: bool,
    reset_pending: bool,
    transition_blocks_controls: bool,
) -> bool {
    advance_pending || step_advance_pending || reset_pending || transition_blocks_controls
}

/// Whether the main window should stay above other windows because the learner
/// must act. The toolbar Start/Continue controls, user-initiated pauses,
/// automatic advance, and automatic playback are deliberately not inputs.
fn learner_action_needed(
    awaiting_human: bool,
    next_action_visible: bool,
    can_retry: bool,
    next_unit_prompt: bool,
    passive_pause: bool,
    passive_waiting: bool,
) -> bool {
    awaiting_human
        || next_action_visible
        || can_retry
        || next_unit_prompt
        || passive_pause
        || passive_waiting
}

fn unit_progress_summary(unit: &cynapse_client_core::UnitView) -> UnitProgressSummary {
    let mut summary = UnitProgressSummary {
        challenges_total: 0,
        challenges_passed: 0,
        challenges_skipped: 0,
        steps_total: unit.steps.len(),
        steps_completed: 0,
        steps_in_progress: 0,
        in_progress_step: None,
    };
    for (index, step) in unit.steps.iter().enumerate() {
        if step.challenge.is_some() {
            summary.challenges_total += 1;
        }
        match step.challenge_status {
            ChallengeStatus::Passed => summary.challenges_passed += 1,
            ChallengeStatus::Skipped => summary.challenges_skipped += 1,
            _ => {}
        }
        if step.completed {
            summary.steps_completed += 1;
        }
        if step.started && !step.completed {
            summary.steps_in_progress += 1;
            summary.in_progress_step.get_or_insert(index + 1);
        }
    }
    summary
}

pub(crate) fn learning_launch_label(status: UnitStatus) -> Option<String> {
    match status {
        UnitStatus::NotStarted => Some(t("unit.start")),
        UnitStatus::InProgress | UnitStatus::Paused => Some(t("unit.continue")),
        UnitStatus::Completed => None,
    }
}

pub(crate) fn retry_advance_after_failure(phase: LearningPhase, at_boundary: bool) -> bool {
    at_boundary && phase == LearningPhase::Failed
}

pub(crate) fn course_identity_matches(
    current: Option<&(String, String)>,
    expected: &(String, String),
) -> bool {
    current == Some(expected)
}

pub(crate) fn continuous_step_auto_advances(requires_human: bool, is_challenge: bool) -> bool {
    is_challenge || !requires_human
}

pub(crate) fn tree_actions_available(is_learning: bool, playback_in_progress: bool) -> bool {
    !is_learning && !playback_in_progress
}

/// Selecting only changes the current target; it must not blink every control.
pub(crate) fn transition_blocks_controls(kind: ClientTransitionKind) -> bool {
    kind != ClientTransitionKind::Selecting
}

/// A committed Unit advance whose plan failed must expose Start again. A
/// pre-commit failure keeps the Failed/Retry path; adapter loss leaves the
/// current state alone (the Connection handler stops adapter-required Units).
pub(crate) fn apply_advance_failure(model: &mut ClientModel, warning: &Warning) {
    model.set_warning(warning.clone());
    match warning {
        Warning::AdapterDisconnected => {}
        Warning::NextUnitPlanUnavailable => model.stop_learning_local(),
        Warning::Message(_) => model.fail_learning(warning.text()),
    }
}

pub(crate) fn start_controls_visible(is_learning: bool, at_unit_boundary: bool) -> bool {
    !is_learning && !at_unit_boundary
}

pub(crate) fn step_advance_label(
    position: &cynapse_client_core::PlaybackPosition,
    has_following_unit: bool,
) -> Option<String> {
    if position.unit_step_index + 1 < position.unit_step_total {
        Some(t("next.step"))
    } else if has_following_unit {
        Some(t("next.unit"))
    } else {
        None
    }
}

/// User intents raised by the view model and handled by the platform layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientIntent {
    ShowCaption,
    HideCaption,
    CaptionFlipped,
    SettingsChanged {
        settings: ClientSettings,
        theme_changed: bool,
        caption_geometry_changed: bool,
    },
}

/// Observable application state for GPUI views.
///
/// A `ClientViewModel` is an `Entity`: views observe it and re-render when it
/// changes. It owns the UI-free [`ClientModel`] and exposes only derived,
/// view-ready values. It does not open windows or touch the OS; those effects
/// belong to `platform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitProgressSummary {
    pub challenges_total: usize,
    pub challenges_passed: usize,
    pub challenges_skipped: usize,
    pub steps_total: usize,
    pub steps_completed: usize,
    pub steps_in_progress: usize,
    /// 1-based Step number currently in progress, if any.
    pub in_progress_step: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NextUnitPromptDismissal {
    course_identity: Option<(String, String)>,
    source_unit_path: String,
    target_unit_path: String,
}

pub struct ClientViewModel {
    model: ClientModel,
    caption_visible: bool,
    caption_text: String,
    caption_formulas_enabled: bool,
    /// Measured window size in logical pixels; the window hugs the caption card.
    caption_width: u32,
    caption_height: u32,
    /// Runtime hover toggle; never written back to the persisted settings.
    caption_flipped: bool,
    /// Current caption band in physical screen pixels.
    caption_rect: Option<(i32, i32, i32, i32)>,
    narration: Option<NarrationService>,
    pending_volume_save: Option<gpui_kit::Task<()>>,
    active_clip: Option<NarrationClipRef>,
    challenge_pending: bool,
    challenge_message: Option<String>,
    challenge_output: Option<String>,
    challenge_exit_code: Option<i32>,
    challenge_output_truncated: bool,
    challenge_needs_save: bool,
    settings_error: Option<String>,
    reset_pending: bool,
    advance_pending: bool,
    step_advance_pending: bool,
    last_auto_step: Option<(String, String)>,
    last_auto_unit: Option<String>,
    last_step_key: Option<(String, String)>,
    next_prompt_dismissed: Option<NextUnitPromptDismissal>,
    next_prompt_mode: Option<LearningMode>,
    course_import: Option<ActiveCourseImport>,
    course_import_error: Option<CourseImportErrorView>,
    course_import_conflict: Option<CourseImportConflictView>,
    next_course_import_error_id: u64,
    next_course_import_conflict_id: u64,
}

impl EventEmitter<ClientIntent> for ClientViewModel {}

/// Course-tree ancestors of a target node, ending with the node itself.
pub(crate) fn breadcrumb_titles(
    nodes: &[cynapse_client_core::CourseTreeNodeView],
    target: &str,
) -> Vec<String> {
    fn walk(
        nodes: &[cynapse_client_core::CourseTreeNodeView],
        target: &str,
        stack: &mut Vec<String>,
        found: &mut Vec<String>,
    ) -> bool {
        for node in nodes {
            stack.push(node.title.clone());
            if node.path == target {
                *found = stack.clone();
                return true;
            }
            if walk(&node.children, target, stack, found) {
                return true;
            }
            stack.pop();
        }
        false
    }

    let mut found = Vec::new();
    walk(nodes, target, &mut Vec::new(), &mut found);
    found
}

impl ClientViewModel {
    pub fn new(
        model: ClientModel,
        events: UnboundedReceiver<ClientEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(model, events, true, cx)
    }

    /// Test seam: skip the audio host so headless tests stay deterministic.
    #[doc(hidden)]
    pub fn new_without_narration(
        model: ClientModel,
        events: UnboundedReceiver<ClientEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(model, events, false, cx)
    }

    fn build(
        model: ClientModel,
        mut events: UnboundedReceiver<ClientEvent>,
        narration_enabled: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let (narration_tx, mut narration_events) = tokio::sync::mpsc::unbounded_channel();
        let narration = narration_enabled
            .then(|| NarrationService::start(narration_tx, model.settings().volume))
            .flatten();
        let session = model.session();

        cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv().await {
                let playback = match &event {
                    ClientEvent::Playback(playback) => Some((**playback).clone()),
                    _ => None,
                };
                let reconnected = matches!(
                    &event,
                    ClientEvent::Connection(ConnectionState::Connected { .. })
                );
                let open_url = match &event {
                    ClientEvent::OpenUrl(url) => Some(url.clone()),
                    _ => None,
                };
                let _ = this.update(cx, |this, cx| {
                    if let Some(url) = open_url {
                        cx.open_url(&url);
                    }
                    let adapter_lost = match &event {
                        ClientEvent::Connection(connection) => {
                            this.model.adapter_loss_ends_learning(connection)
                        }
                        ClientEvent::Playback(playback) => {
                            this.model.playback_marks_adapter_loss(playback)
                        }
                        _ => false,
                    };
                    if reconnected {
                        this.model.clear_warnings();
                        this.challenge_message = None;
                    }
                    this.model.apply(event);
                    if let Some(playback) = playback.as_ref() {
                        this.handle_playback(playback, cx);
                    }
                    if adapter_lost {
                        this.stop_learning(cx);
                    }
                    this.maybe_auto_advance(cx);
                    cx.notify();
                });
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            while let Some(event) = narration_events.recv().await {
                match event {
                    NarrationEvent::Started { clip } => {
                        let _ = session.narration_started(clip);
                    }
                    NarrationEvent::Ended { clip } => {
                        let _ = this.update(cx, |this, cx| {
                            if this.active_clip.as_ref() == Some(&clip) {
                                let _ = session.narration_ended(clip);
                                this.active_clip = None;
                                this.set_caption(None, cx);
                            }
                        });
                    }
                    NarrationEvent::Failed { clip, message } => {
                        let _ = this.update(cx, |this, cx| {
                            if this.active_clip.as_ref() == Some(&clip) {
                                let _ = session.narration_failed(clip, message.clone());
                                this.active_clip = None;
                                this.set_caption(None, cx);
                                this.model.set_warning(message);
                                cx.notify();
                            }
                        });
                    }
                }
            }
        })
        .detach();

        let mut view = Self {
            model,
            caption_visible: false,
            caption_text: String::new(),
            caption_formulas_enabled: true,
            caption_width: crate::caption_metrics::CAPTION_MIN_WIDTH as u32,
            caption_height: crate::caption_metrics::CAPTION_MIN_HEIGHT as u32,
            caption_flipped: false,
            caption_rect: None,
            narration,
            pending_volume_save: None,
            active_clip: None,
            challenge_pending: false,
            challenge_message: None,
            challenge_output: None,
            challenge_exit_code: None,
            challenge_output_truncated: false,
            challenge_needs_save: false,
            settings_error: None,
            reset_pending: false,
            advance_pending: false,
            step_advance_pending: false,
            last_auto_step: None,
            last_auto_unit: None,
            last_step_key: None,
            next_prompt_dismissed: None,
            next_prompt_mode: None,
            course_import: None,
            course_import_error: None,
            course_import_conflict: None,
            next_course_import_error_id: 0,
            next_course_import_conflict_id: 0,
        };
        view.maybe_auto_advance(cx);
        view
    }

    pub fn course_title(&self) -> Option<String> {
        self.model.course_title().map(str::to_string)
    }

    pub fn course_content_revision(&self) -> u64 {
        self.model.course_content_revision()
    }

    pub fn course_tree(&self) -> &[cynapse_client_core::CourseTreeNodeView] {
        self.model.course_tree()
    }

    pub fn course_rows(
        &self,
    ) -> std::sync::Arc<std::collections::HashMap<String, ProjectedCourseRow>> {
        self.model.course_rows()
    }

    pub fn current_position(&self) -> Option<&cynapse_client_core::PlaybackPosition> {
        self.model.current_position()
    }

    pub fn has_progress(&self) -> bool {
        self.model.has_progress()
    }

    pub fn current_unit_progress(&self) -> Option<UnitProgressSummary> {
        let path = self.model.selected_unit_path()?;
        let unit = cynapse_client_core::find_unit_by_path(self.course_tree(), path)?;
        Some(unit_progress_summary(unit))
    }

    pub fn units(&self) -> &[UnitRow] {
        self.model.units()
    }

    pub fn selected_unit_path(&self) -> Option<&str> {
        self.model.selected_unit_path()
    }

    /// Course-tree ancestors of the selected Unit, ending with the Unit itself.
    pub fn selected_unit_breadcrumb(&self) -> Vec<String> {
        let Some(target) = self.selected_unit_path() else {
            return Vec::new();
        };

        let mut found = breadcrumb_titles(self.course_tree(), target);
        if found.is_empty()
            && let Some(unit) = self.units().iter().find(|unit| unit.path == target)
        {
            found.push(unit.title.clone());
        }
        found
    }

    pub fn recent_courses(&self) -> Vec<RecentCourse> {
        self.model.recent_courses().to_vec()
    }

    pub fn is_learning(&self) -> bool {
        self.model.is_learning()
    }

    /// A failed Stop can leave Playback running after the local learning mode
    /// was cleared. The controls follow Playback so the toolbar still offers Stop.
    pub fn learning_active(&self) -> bool {
        self.is_learning() || self.playback_active()
    }

    pub fn proxy_stop_label(&self) -> String {
        if self.transition_is_finalizing() {
            t("playback.stopping")
        } else {
            t("playback.stop")
        }
    }

    pub fn stop_control_enabled(&self) -> bool {
        self.learning_active()
            && self.selected_unit_path().is_some()
            && !self.transition_is_finalizing()
    }

    fn transition_is_finalizing(&self) -> bool {
        self.model
            .state()
            .transition()
            .is_some_and(|transition| transition.phase == ClientTransitionPhase::Finalizing)
    }

    pub fn start_controls_visible(&self) -> bool {
        self.selected_unit_path().is_some()
            && start_controls_visible(
                self.learning_active(),
                self.at_unit_boundary() || self.at_completed_unit_cursor(),
            )
    }

    /// The next Unit needs the learner's decision before anything changes.
    /// Covers both a completed playback boundary and a completed Unit cursor.
    pub fn pending_next_unit(&self) -> Option<UnitRow> {
        if self.is_learning() {
            return None;
        }
        let target = match self.model.next_target() {
            Some(NextTarget::Unit(target)) => target,
            _ => return None,
        };
        let unit = target.prompted_unit(self.units())?;
        if self
            .next_prompt_dismissed
            .as_ref()
            .is_some_and(|dismissal| {
                dismissal.course_identity == self.model.course_identity()
                    && dismissal.source_unit_path == target.source
                    && dismissal.target_unit_path == target.path
            })
        {
            return None;
        }
        Some(unit.clone())
    }

    pub fn cancel_next_unit(
        &mut self,
        course_identity: Option<(String, String)>,
        source_unit_path: Option<String>,
        target_unit_path: String,
        cx: &mut Context<Self>,
    ) {
        let Some(NextTarget::Unit(target)) = self.model.next_target() else {
            return;
        };
        if self.course_identity() != course_identity
            || Some(target.source.as_str()) != source_unit_path.as_deref()
            || target.path != target_unit_path
        {
            tracing::debug!(
                target: "cynapse.client.flow",
                target_unit_path,
                "stale next-unit cancel ignored"
            );
            return;
        }
        self.next_prompt_mode = None;
        self.next_prompt_dismissed = Some(NextUnitPromptDismissal {
            course_identity,
            source_unit_path: target.source,
            target_unit_path: target.path,
        });
        cx.notify();
    }

    pub fn learning_mode_label(&self) -> Option<String> {
        match self.model.state().learning().mode {
            Some(LearningMode::ByStep) => Some(t("learning.mode.by_step")),
            Some(LearningMode::Continuous) => Some(t("learning.mode.continuous")),
            None => None,
        }
    }

    /// True while playback waits for the learner because the current Step
    /// requires human input or is a Challenge.
    pub fn awaiting_human(&self) -> bool {
        self.is_learning() && awaiting_human_state(self.model.state().playback())
    }

    /// True while the main window should stay above other windows because the
    /// learner must act: a human/Challenge wait, a visible Next/Next Unit
    /// button, Retry, the next-Unit prompt, a passive playback interruption, or
    /// a disconnected Adapter during an active wait. The toolbar Start/Continue
    /// controls, user-initiated pauses, automatic advance, and automatic
    /// playback are excluded.
    pub fn awaiting_learner_action(&self) -> bool {
        if learner_action_pending(
            self.advance_pending,
            self.step_advance_pending,
            self.reset_pending,
            self.transition_blocks_controls(),
        ) {
            return false;
        }
        let playback = self.model.state().playback();
        // A passive wait only counts while a Learning Mode is active. Stop
        // leaves the last Playback state behind (it is not Playing, so nothing
        // overwrites it), and reading that leftover as a wait re-showed the
        // window from the tray the moment the idle Adapter's heartbeat dropped.
        let learning = self.is_learning();
        learner_action_needed(
            self.awaiting_human(),
            self.next_action_visible(&self.next_action()),
            self.can_retry(),
            self.pending_next_unit().is_some(),
            learning && has_passive_pause(playback),
            learning && passive_wait_needs_learner(self.adapter_ready(), playback),
        )
    }

    /// Identity of the current learner wait. A new key means a new reminder,
    /// even when the previous wait is still topmost.
    #[doc(hidden)]
    pub fn learner_wait_key(&self) -> Option<LearnerWaitKey> {
        if !self.awaiting_learner_action() {
            return None;
        }
        Some(LearnerWaitKey {
            playback: self.model.state().playback().clone(),
            next_target: self.model.next_target(),
            prompt_target: self.pending_next_unit().map(|unit| unit.path),
            adapter_ready: self.adapter_ready(),
        })
    }

    pub fn playback_active(&self) -> bool {
        matches!(
            self.model.state().playback(),
            PlaybackState::Playing { .. } | PlaybackState::Narrating { .. }
        )
    }

    pub fn tree_actions_available(&self) -> bool {
        tree_actions_available(self.is_learning(), self.playback_active())
    }

    pub fn transition_label(&self) -> Option<String> {
        let state = self.model.state();
        if let Some(transition) = state.transition() {
            return transition
                .message
                .clone()
                .or_else(|| Some(transition_kind_label(transition.kind)));
        }
        state.pending_transition().map(|transition| {
            let message = transition
                .message
                .clone()
                .unwrap_or_else(|| transition_kind_label(transition.kind));
            t_with("playback.queued", &[("message", &message)])
        })
    }

    pub fn startup_visible(&self) -> bool {
        self.model
            .state()
            .transition()
            .is_some_and(|transition| transition.kind == ClientTransitionKind::Starting)
    }

    pub fn startup_stage(&self) -> Option<ClientTransitionStage> {
        self.model
            .state()
            .transition()
            .filter(|transition| transition.kind == ClientTransitionKind::Starting)
            .and_then(|transition| transition.stage)
    }

    /// Labels and active index for the shared busy overlay. The order is the
    /// visible startup order, independent of which Adapter the Unit uses.
    pub fn startup_steps(&self) -> (Vec<String>, Option<usize>) {
        let steps = [
            (
                ClientTransitionStage::PrepareEnvironment,
                t("startup.step.environment"),
            ),
            (
                ClientTransitionStage::ConnectAdapter,
                t("startup.step.connection"),
            ),
            (
                ClientTransitionStage::CheckEnvironment,
                t("startup.step.check"),
            ),
            (
                ClientTransitionStage::PrepareWorkspace,
                t("startup.step.workspace"),
            ),
            (
                ClientTransitionStage::StartPlayback,
                t("startup.step.playback"),
            ),
        ];
        let active = self
            .startup_stage()
            .and_then(|stage| steps.iter().position(|(candidate, _)| *candidate == stage));
        (steps.into_iter().map(|(_, label)| label).collect(), active)
    }

    pub fn transition_blocks_controls(&self) -> bool {
        let state = self.model.state();
        state
            .transition()
            .is_some_and(|transition| transition_blocks_controls(transition.kind))
            || state
                .pending_transition()
                .is_some_and(|transition| transition_blocks_controls(transition.kind))
    }

    pub fn adapter_status(&self) -> AdapterStatus {
        let unit_path = self.model.selected_unit_path();
        let adapters = unit_path
            .and_then(|path| self.units().iter().find(|unit| unit.path == path))
            .map(|unit| unit.adapters.clone())
            .unwrap_or_default();
        adapter_status_for(&adapters, self.model.state().connection())
    }

    pub fn adapter_ready(&self) -> bool {
        let status = self.adapter_status();
        !status.required || status.connected
    }

    pub fn current_challenge(&self) -> Option<&ChallengeView> {
        self.model.current_challenge()
    }

    pub fn challenge_status(&self) -> ChallengeStatus {
        self.model
            .current_challenge_status()
            .unwrap_or(ChallengeStatus::NotStarted)
    }

    pub fn challenge_attempts(&self) -> u32 {
        self.model.current_challenge_attempts()
    }

    pub fn challenge_pending(&self) -> bool {
        self.challenge_pending
    }

    pub fn challenge_message(&self) -> Option<&str> {
        self.challenge_message.as_deref()
    }

    pub fn challenge_output(&self) -> Option<&str> {
        self.challenge_output.as_deref()
    }

    pub fn challenge_exit_code(&self) -> Option<i32> {
        self.challenge_exit_code
    }

    pub fn challenge_output_truncated(&self) -> bool {
        self.challenge_output_truncated
    }

    pub fn challenge_needs_save(&self) -> bool {
        self.challenge_needs_save
    }

    /// The automatic path owns this StepComplete boundary; a manual Next Step
    /// button would only flash until the automatic advance fires.
    fn auto_step_will_run(&self) -> bool {
        let PlaybackState::StepComplete {
            position,
            continuation: None,
            ..
        } = self.model.state().playback()
        else {
            return false;
        };
        if !self.model.can_advance_step() {
            return false;
        }
        let key = (position.unit_path.clone(), position.step_id.clone());
        if self.last_auto_step.as_ref() == Some(&key) {
            return false;
        }
        let current = self.model.current_step();
        let is_challenge = current.is_some_and(|step| step.challenge.is_some());
        let requires_human = current.is_some_and(|step| step.requires_human);
        self.model.at_final_challenge_checkpoint()
            || (self.model.state().learning().mode == Some(LearningMode::Continuous)
                && continuous_step_auto_advances(requires_human, is_challenge))
    }

    pub fn at_checkpoint(&self) -> bool {
        self.model.at_checkpoint()
    }

    fn at_step_boundary(&self) -> bool {
        self.model.at_step_boundary()
    }

    pub fn checkpoint_position_key(&self) -> Option<(String, String)> {
        if !self.at_checkpoint() {
            return None;
        }
        self.model
            .current_position()
            .map(|position| (position.unit_path.clone(), position.step_id.clone()))
    }

    pub fn next_unit(&self) -> Option<UnitRow> {
        self.model.next_unit().cloned()
    }

    pub fn next_unit_source_path(&self) -> Option<String> {
        match self.model.next_target() {
            Some(NextTarget::Unit(target)) => Some(target.source),
            _ => None,
        }
    }

    pub fn prepare_next_unit_if_current(
        &mut self,
        course_identity: Option<(String, String)>,
        source_unit_path: Option<String>,
        target_unit_path: String,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        let target_is_next = matches!(
            self.model.next_target(),
            Some(NextTarget::Unit(target))
                if target.path == target_unit_path
                    && Some(target.source.as_str()) == source_unit_path.as_deref()
        );
        if self.course_identity() != course_identity || !target_is_next {
            tracing::debug!(
                target: "cynapse.client.flow",
                target_unit_path,
                "stale next-unit confirmation ignored"
            );
            self.model.set_warning(t("warning.target_unit_changed"));
            cx.notify();
            return;
        }
        self.prepare_next_unit(target_unit_path, reset, cx);
    }

    pub fn advance_label(&self) -> Option<String> {
        let position = self.model.current_position()?;
        step_advance_label(position, self.next_unit().is_some())
    }

    /// One decision for what comes next. The target is logical; `can_run`
    /// remains a separate gate for adapters and in-flight transitions.
    pub(crate) fn next_action(&self) -> NextAction {
        match self.model.next_target() {
            Some(NextTarget::Step) => NextAction {
                target: Some(NextTarget::Step),
                automatic: self.auto_step_will_run(),
                prompt: false,
            },
            Some(NextTarget::Unit(target)) => {
                let prompt = target.prompted_unit(self.units()).is_some();
                NextAction {
                    target: Some(NextTarget::Unit(target.clone())),
                    automatic: target.origin == NextUnitOrigin::Boundary
                        && self.auto_unit_will_run(),
                    prompt,
                }
            }
            None => NextAction::none(),
        }
    }

    pub(crate) fn can_run_next_action(&self, action: &NextAction) -> bool {
        match action.target.as_ref() {
            Some(NextTarget::Step) => {
                self.is_learning()
                    && self.adapter_ready()
                    && self.at_step_boundary()
                    && self.model.can_advance_step()
                    && !self.advance_pending
                    && !self.challenge_pending
                    && !self.step_advance_pending
                    && !self.transition_blocks_controls()
            }
            Some(NextTarget::Unit(target)) => match target.origin {
                NextUnitOrigin::Boundary => self.can_advance_unit(),
                NextUnitOrigin::Cursor => self.can_prepare_next_unit(),
            },
            None => false,
        }
    }

    pub(crate) fn next_action_visible(&self, action: &NextAction) -> bool {
        action.target.is_some()
            && !action.prompt
            && !action.automatic
            && self.can_run_next_action(action)
    }

    pub fn at_unit_boundary(&self) -> bool {
        self.model.at_unit_boundary()
    }

    pub fn at_completed_unit_cursor(&self) -> bool {
        self.model.at_completed_unit_cursor()
    }

    pub fn can_advance_unit(&self) -> bool {
        self.at_unit_boundary()
            && !self.advance_pending
            && !self.reset_pending
            && !self.transition_blocks_controls()
            && self.next_unit().is_some()
    }

    fn can_prepare_next_unit(&self) -> bool {
        self.at_completed_unit_cursor()
            && !self.is_learning()
            && !self.advance_pending
            && !self.reset_pending
            && !self.challenge_pending
            && !self.transition_blocks_controls()
    }

    fn auto_unit_will_run(&self) -> bool {
        let PlaybackState::UnitComplete { position } = self.model.state().playback() else {
            return false;
        };
        if self.last_auto_unit.as_ref() == Some(&position.unit_path) {
            return false;
        }
        prompted_next_unit(self.units(), &position.unit_path).is_none()
    }

    pub fn advance_pending(&self) -> bool {
        self.advance_pending
    }

    pub fn can_retry(&self) -> bool {
        self.adapter_ready()
            && (self.model.can_retry()
                || retry_advance_after_failure(
                    self.model.state().learning().phase,
                    self.at_unit_boundary(),
                ))
            && !self.advance_pending
            && !self.reset_pending
            && !self.step_advance_pending
            && !self.transition_blocks_controls()
    }

    pub fn playback_label(&self) -> Option<String> {
        match self.model.state().playback() {
            PlaybackState::Idle => None,
            PlaybackState::Ready { position } => Some(
                position
                    .as_ref()
                    .map(|position| {
                        t_with("playback.ready_step", &[("step", &position.step_title)])
                    })
                    .unwrap_or_else(|| t("playback.ready")),
            ),
            PlaybackState::Playing {
                action_label,
                position,
                ..
            } => Some(t_with(
                "playback.executing",
                &[("step", &position.step_title), ("action", action_label)],
            )),
            PlaybackState::Narrating { position, .. } => Some(t_with(
                "playback.narrating",
                &[("step", &position.step_title)],
            )),
            PlaybackState::Paused {
                position,
                interruption,
            } => {
                let key = if interruption.reason
                    == PlaybackInterruptionReason::JupyterLabKernelRestarted
                {
                    "playback.jupyterlab_kernel_restarted"
                } else {
                    "playback.stopped_at"
                };
                Some(t_with(key, &[("step", &position.step_title)]))
            }
            PlaybackState::StepComplete { position, .. } => Some(t_with(
                "playback.step_complete",
                &[("step", &position.step_title)],
            )),
            PlaybackState::UnitComplete { position } => Some(t_with(
                "playback.unit_complete",
                &[("unit", &position.unit_title)],
            )),
            PlaybackState::Error { message, .. } => {
                Some(t_with("playback.error", &[("message", message)]))
            }
        }
    }

    pub fn current_step_title(&self) -> Option<String> {
        let position = match self.model.state().playback() {
            PlaybackState::Ready { position } => position.as_ref()?,
            PlaybackState::Playing { position, .. }
            | PlaybackState::Narrating { position, .. }
            | PlaybackState::Paused { position, .. }
            | PlaybackState::StepComplete { position, .. } => position,
            PlaybackState::Error { position, .. } => position.as_ref()?,
            PlaybackState::Idle | PlaybackState::UnitComplete { .. } => return None,
        };
        Some(position.step_title.clone())
    }

    pub fn warning_label(&self) -> Option<String> {
        let warnings = self.model.state().warnings();
        if warnings.is_empty() {
            None
        } else {
            Some(
                warnings
                    .iter()
                    .map(Warning::text)
                    .collect::<Vec<_>>()
                    .join(&t("list.separator")),
            )
        }
    }

    pub fn learning_error(&self) -> Option<&str> {
        self.model.state().learning().error.as_deref()
    }

    pub fn course_import_view(&self) -> Option<CourseImportView> {
        self.course_import
            .as_ref()
            .map(|import| import.view.clone())
    }

    pub fn course_import_error(&self) -> Option<&CourseImportErrorView> {
        self.course_import_error.as_ref()
    }

    pub fn course_import_conflict(&self) -> Option<&CourseImportConflictView> {
        self.course_import_conflict.as_ref()
    }

    pub fn course_import_target(&self, url: &str) -> Result<Option<PathBuf>, String> {
        let store_dir = self.settings().resolved_course_store_dir();
        let target = cynapse_client_core::course_import::suggested_target(&store_dir, url)
            .map_err(|error| error.to_string())?;
        Ok(target.filter(|path| path.exists()))
    }

    pub fn is_active_course_path(&self, path: &Path) -> bool {
        let Some((_, active)) = self.model.course_identity() else {
            return false;
        };
        let active = Path::new(&active);
        if active == path {
            return true;
        }
        let active = std::fs::canonicalize(active).unwrap_or_else(|_| active.to_path_buf());
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        paths_equal(&active, &target)
    }

    pub fn start_course_import(
        &mut self,
        url: String,
        confirmed_target: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.course_import.is_some() {
            return;
        }
        if let Err(error) = cynapse_client_core::course_import::validate_url(&url) {
            self.set_course_import_error(error.to_string(), cx);
            return;
        }
        let store_dir = self.settings().resolved_course_store_dir();
        if confirmed_target
            .as_deref()
            .is_some_and(|target| self.is_active_course_path(target))
        {
            self.set_course_import_error(t("course_import.error.active_course_overwrite"), cx);
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let view = CourseImportView {
            label: t("course_import.opening"),
            steps: vec![
                t("course_import.step.download"),
                t("course_import.step.extract"),
                t("course_import.step.install"),
            ],
            active_step: Some(0),
            step_details: vec![None, None, None],
            cancellable: true,
        };
        self.course_import = Some(ActiveCourseImport {
            view,
            cancel: cancel.clone(),
            url: url.clone(),
        });
        self.course_import_error = None;
        self.course_import_conflict = None;
        self.model.clear_warnings();
        cx.notify();

        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<CourseImportProgress>();
        cx.spawn(async move |this, cx| {
            while let Some(progress) = progress_rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    this.apply_course_import_progress(progress, cx)
                });
            }
        })
        .detach();

        let cancel_for_task = cancel;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut progress = move |progress: CourseImportProgress| {
                        let _ = progress_tx.send(progress);
                    };
                    cynapse_client_core::course_import::import_course(
                        &url,
                        &store_dir,
                        confirmed_target.as_deref(),
                        &cancel_for_task,
                        &mut progress,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| this.finish_course_import(result, cx));
        })
        .detach();
    }

    pub fn cancel_course_import(&mut self, cx: &mut Context<Self>) {
        let Some(import) = self.course_import.as_mut() else {
            return;
        };
        import.cancel.store(true, Ordering::Relaxed);
        import.view.cancellable = false;
        let active = import.view.active_step.unwrap_or(0);
        if let Some(detail) = import.view.step_details.get_mut(active) {
            *detail = Some(t("course_import.cancelling"));
        }
        cx.notify();
    }

    fn apply_course_import_progress(
        &mut self,
        progress: CourseImportProgress,
        cx: &mut Context<Self>,
    ) {
        let Some(import) = self.course_import.as_mut() else {
            return;
        };
        import.view.active_step = Some(match progress.stage {
            CourseImportStage::Downloading => 0,
            CourseImportStage::Extracting => 1,
            CourseImportStage::Installing => 2,
        });
        import.view.cancellable = !import.cancel.load(Ordering::Relaxed);
        match progress.stage {
            CourseImportStage::Downloading => {
                import.view.step_details[0] = Some(match progress.percent {
                    Some(percent) => format!(
                        "{}% · {}",
                        percent,
                        format_download_size(progress.downloaded, progress.total)
                    ),
                    None => format_download_size(progress.downloaded, progress.total),
                });
            }
            CourseImportStage::Extracting | CourseImportStage::Installing => {
                import.view.step_details.fill(None);
            }
        }
        cx.notify();
    }

    fn finish_course_import(
        &mut self,
        result: Result<PathBuf, cynapse_client_core::course_import::CourseImportError>,
        cx: &mut Context<Self>,
    ) {
        let url = self
            .course_import
            .as_ref()
            .map(|import| import.url.clone())
            .unwrap_or_default();
        self.course_import = None;
        match result {
            Ok(path) => self.open_course(path, cx),
            Err(cynapse_client_core::course_import::CourseImportError::Cancelled) => {
                self.model.clear_warnings();
                cx.notify();
            }
            Err(cynapse_client_core::course_import::CourseImportError::AlreadyExists(target)) => {
                self.next_course_import_conflict_id =
                    self.next_course_import_conflict_id.saturating_add(1);
                self.course_import_conflict = Some(CourseImportConflictView {
                    id: self.next_course_import_conflict_id,
                    url,
                    target,
                });
                cx.notify();
            }
            Err(error) => {
                self.model.clear_warnings();
                self.next_course_import_error_id =
                    self.next_course_import_error_id.saturating_add(1);
                self.course_import_error = Some(CourseImportErrorView {
                    id: self.next_course_import_error_id,
                    message: error.to_string(),
                });
                cx.notify();
            }
        }
    }

    fn set_course_import_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.next_course_import_error_id = self.next_course_import_error_id.saturating_add(1);
        self.course_import_error = Some(CourseImportErrorView {
            id: self.next_course_import_error_id,
            message,
        });
        cx.notify();
    }

    pub fn open_course(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.clear_challenge_transient();
        self.model.set_pending_learning_intent(None);
        self.next_prompt_dismissed = None;
        self.next_prompt_mode = None;
        self.last_auto_step = None;
        self.last_auto_unit = None;
        self.last_step_key = None;
        self.step_advance_pending = false;
        self.advance_pending = false;
        self.reset_pending = false;
        self.challenge_pending = false;
        self.model.stop_learning_local();
        self.model
            .apply(ClientEvent::Playback(Box::new(PlaybackState::Idle)));
        self.model.clear_warnings();
        self.stop_narration(cx);
        self.run_session_op(cx, |session| async move { session.open_course(path).await });
    }

    pub fn clear_recent_courses(&mut self, cx: &mut Context<Self>) {
        self.model.clear_warnings();
        self.run_session_op(cx, |session| async move { session.clear_recent_courses() });
    }

    pub fn close_course(&mut self, cx: &mut Context<Self>) {
        self.next_prompt_dismissed = None;
        self.next_prompt_mode = None;
        self.last_auto_step = None;
        self.last_auto_unit = None;
        self.step_advance_pending = false;
        self.advance_pending = false;
        self.reset_pending = false;
        self.challenge_pending = false;
        self.challenge_message = None;
        self.challenge_output = None;
        self.challenge_needs_save = false;
        self.model.stop_learning_local();
        self.model.clear_warnings();
        self.model.set_pending_learning_intent(None);
        self.stop_narration(cx);
        self.run_session_op(cx, |session| async move { session.close_course().await });
    }

    pub fn select_unit(&mut self, unit_path: String, cx: &mut Context<Self>) {
        self.next_prompt_mode = None;
        self.select_unit_with_mode(unit_path, None, cx);
    }

    fn select_unit_with_mode(
        &mut self,
        unit_path: String,
        resume_mode: Option<LearningMode>,
        cx: &mut Context<Self>,
    ) {
        tracing::debug!(
            target: "cynapse.client.flow",
            %unit_path,
            ?resume_mode,
            "selecting unit"
        );
        self.next_prompt_dismissed = None;
        self.model.clear_warnings();
        self.model.set_pending_learning_intent(None);
        self.clear_challenge_transient();
        self.run_session_op_with(
            cx,
            move |session| async move { session.select_unit(&unit_path).await },
            move |this, result, cx| this.finish_selection(result, resume_mode, cx),
        );
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.next_prompt_mode = None;
        self.next_prompt_dismissed = None;
        self.model.clear_warnings();
        self.model.set_pending_learning_intent(None);
        self.clear_challenge_transient();
        self.run_session_op(cx, |session| async move { session.clear_selection().await });
    }

    pub fn start_learning(&mut self, mode: LearningMode, cx: &mut Context<Self>) {
        let Some(unit_path) = self.model.selected_unit_path().map(str::to_string) else {
            self.model.set_warning(t("warning.select_unit"));
            return;
        };
        self.next_prompt_dismissed = None;
        self.next_prompt_mode = None;
        self.model.set_pending_learning_intent(None);
        self.clear_challenge_transient();
        tracing::debug!(
            target: "cynapse.client.flow",
            %unit_path,
            ?mode,
            "starting learning"
        );
        self.last_auto_step = None;
        self.last_auto_unit = None;
        self.step_advance_pending = false;
        if let Err(error) = self.model.start_learning(mode, &unit_path) {
            self.model.set_warning(error);
            return;
        }
        let generation = self.model.learning_generation();
        let session = self.model.session();
        cx.spawn(async move |this, cx| {
            if let Err(error) = session.begin_learning(mode, &unit_path).await {
                tracing::warn!(
                    target: "cynapse.client.flow",
                    %unit_path,
                    %error,
                    "could not start learning"
                );
                let _ = this.update(cx, |this, cx| {
                    if this.model.stop_learning_after_start_failure(generation) {
                        this.model.set_warning(error);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn stop_learning(&mut self, cx: &mut Context<Self>) {
        tracing::debug!(
            target: "cynapse.client.flow",
            unit_path = self.model.selected_unit_path().unwrap_or("none"),
            "stopping learning"
        );
        self.last_auto_step = None;
        self.last_auto_unit = None;
        self.step_advance_pending = false;
        self.next_prompt_mode = None;
        self.model.set_pending_learning_intent(None);
        if self
            .model
            .current_step()
            .is_some_and(|step| !step.completed)
            && let Some(position) = self.model.current_position()
        {
            let resume = (
                position.unit_path.clone(),
                position.step_id.clone(),
                position.action_index,
            );
            if let Err(error) = self
                .model
                .session()
                .record_resume_action(&resume.0, &resume.1, resume.2)
            {
                tracing::warn!(
                    target: "cynapse.client.flow",
                    %error,
                    "could not record the resume action"
                );
            }
        }
        self.stop_narration(cx);
        self.model.stop_learning_local();
        // Stop must clear the wait and topmost before the async stop returns.
        cx.notify();
        let session = self.model.session();
        cx.spawn(async move |this, cx| {
            if let Err(error) = session.stop_learning().await {
                let _ = this.update(cx, |this, cx| {
                    this.model.set_warning(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn reset_pending(&self) -> bool {
        self.reset_pending
    }

    /// Stop is a Client transition shared by every Adapter. JupyterLab can
    /// spend several seconds cancelling a Cell, so the same busy overlay is
    /// shown while the transition is finalizing.
    pub fn stop_pending(&self) -> bool {
        self.model
            .state()
            .transition()
            .is_some_and(|transition| transition.kind == ClientTransitionKind::Stopping)
    }

    /// Test seam: drive the MainView overlay without running a destructive reset.
    #[doc(hidden)]
    pub fn set_reset_pending_for_test(&mut self, pending: bool, cx: &mut Context<Self>) {
        self.reset_pending = pending;
        cx.notify();
    }

    pub fn can_reset_course(&self) -> bool {
        self.model.has_progress()
    }

    pub fn session(&self) -> cynapse_client_model::SessionService {
        self.model.session()
    }

    pub fn course_identity(&self) -> Option<(String, String)> {
        self.model.course_identity()
    }

    pub fn reset_unit_if_current(
        &mut self,
        identity: (String, String),
        unit_path: String,
        cx: &mut Context<Self>,
    ) {
        if course_identity_matches(self.model.course_identity().as_ref(), &identity)
            && self.units().iter().any(|unit| unit.path == unit_path)
        {
            self.reset_unit(unit_path, cx);
        } else {
            tracing::debug!(
                target: "cynapse.client.flow",
                unit_path,
                "stale unit reset confirmation ignored"
            );
            self.model
                .set_warning(t("warning.target_changed_reset_cancelled"));
            cx.notify();
        }
    }

    pub fn reset_course_if_current(&mut self, identity: (String, String), cx: &mut Context<Self>) {
        if course_identity_matches(self.model.course_identity().as_ref(), &identity) {
            self.reset_course(cx);
        } else {
            tracing::debug!(
                target: "cynapse.client.flow",
                "stale course reset confirmation ignored"
            );
            self.model
                .set_warning(t("warning.course_changed_reset_cancelled"));
            cx.notify();
        }
    }

    pub fn close_course_if_current(&mut self, identity: (String, String), cx: &mut Context<Self>) {
        if course_identity_matches(self.model.course_identity().as_ref(), &identity) {
            self.close_course(cx);
        } else {
            tracing::debug!(
                target: "cynapse.client.flow",
                "stale close-course confirmation ignored"
            );
            self.model
                .set_warning(t("warning.course_changed_close_cancelled"));
            cx.notify();
        }
    }

    pub fn skip_challenge_if_current(
        &mut self,
        unit_path: String,
        step_id: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .model
            .current_position()
            .is_some_and(|position| position.unit_path == unit_path && position.step_id == step_id)
        {
            self.skip_challenge(cx);
        } else {
            tracing::debug!(
                target: "cynapse.client.flow",
                unit_path,
                step_id,
                "stale challenge skip confirmation ignored"
            );
        }
    }

    pub fn reset_unit(&mut self, unit_path: String, cx: &mut Context<Self>) {
        if self.reset_pending || self.challenge_pending {
            return;
        }
        self.begin_reset(cx);
        self.run_session_op_with(
            cx,
            move |session| async move { session.reset_unit(&unit_path).await },
            |this, result, cx| {
                this.reset_pending = false;
                this.apply_course_result(result, cx);
            },
        );
    }

    /// Prepares the next Unit without starting playback. Resetting is optional
    /// because a partially studied Unit may be continued instead.
    pub fn prepare_next_unit(&mut self, unit_path: String, reset: bool, cx: &mut Context<Self>) {
        tracing::debug!(
            target: "cynapse.client.flow",
            %unit_path,
            reset,
            "preparing next unit"
        );
        if self.reset_pending || self.advance_pending || self.challenge_pending {
            return;
        }
        self.next_prompt_dismissed = None;
        let resume_mode = self.next_prompt_mode.take();
        if !reset {
            self.stop_narration(cx);
            self.select_unit_with_mode(unit_path, resume_mode, cx);
            return;
        }
        self.begin_reset(cx);
        self.run_session_op_with(
            cx,
            move |session| async move {
                session.reset_unit(&unit_path).await?;
                session.select_unit(&unit_path).await
            },
            move |this, result, cx| {
                this.reset_pending = false;
                this.finish_selection(result, resume_mode, cx);
            },
        );
    }

    pub fn reset_course(&mut self, cx: &mut Context<Self>) {
        if self.reset_pending || self.challenge_pending {
            return;
        }
        self.begin_reset(cx);
        self.run_session_op_with(
            cx,
            |session| async move { session.reset_course().await },
            |this, result, cx| {
                this.reset_pending = false;
                this.apply_course_result(result, cx);
            },
        );
    }

    fn begin_reset(&mut self, cx: &mut Context<Self>) {
        self.reset_pending = true;
        self.advance_pending = false;
        self.step_advance_pending = false;
        self.next_prompt_mode = None;
        self.next_prompt_dismissed = None;
        self.last_auto_step = None;
        self.last_auto_unit = None;
        self.challenge_pending = false;
        self.clear_challenge_transient();
        self.model.set_pending_learning_intent(None);
        self.model.stop_learning_local();
        self.stop_narration(cx);
        cx.notify();
    }

    /// Runs one course-session call off the UI thread and applies the outcome
    /// on it. Every course operation above is this shape.
    fn run_session_op<Op, Fut>(&mut self, cx: &mut Context<Self>, op: Op)
    where
        Op: FnOnce(SessionService) -> Fut + 'static,
        Fut: Future<Output = Result<CourseSessionState, String>> + Send + 'static,
    {
        self.run_session_op_with(cx, op, Self::apply_course_result);
    }

    /// Runs one course-session call and lets `done` decide what the outcome
    /// means once it is back on the UI thread.
    fn run_session_op_with<Op, Fut, Done>(&mut self, cx: &mut Context<Self>, op: Op, done: Done)
    where
        Op: FnOnce(SessionService) -> Fut + 'static,
        Fut: Future<Output = Result<CourseSessionState, String>> + Send + 'static,
        Done: FnOnce(&mut Self, Result<CourseSessionState, String>, &mut Context<Self>) + 'static,
    {
        let session = self.model.session();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(op(session)).await;
            let _ = this.update(cx, |this, cx| done(this, result, cx));
        })
        .detach();
    }

    /// The shared tail of every course operation: the new state replaces the
    /// current one, or the failure becomes the visible warning.
    fn apply_course_result(
        &mut self,
        result: Result<CourseSessionState, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(state) => self.model.apply_course_state(state),
            Err(error) => self.model.set_warning(error),
        }
        cx.notify();
    }

    /// Selection carries a resume mode: a success starts playback in it, a
    /// failure puts it back so the prompt can offer the same choice again.
    fn finish_selection(
        &mut self,
        result: Result<CourseSessionState, String>,
        resume_mode: Option<LearningMode>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(state) => {
                self.model.apply_course_state(state);
                if let Some(mode) = resume_mode {
                    self.start_learning(mode, cx);
                }
                cx.notify();
            }
            Err(error) => {
                self.next_prompt_mode = resume_mode;
                self.apply_course_result(Err(error), cx);
            }
        }
    }

    fn clear_challenge_transient(&mut self) {
        self.challenge_message = None;
        self.challenge_output = None;
        self.challenge_exit_code = None;
        self.challenge_output_truncated = false;
        self.challenge_needs_save = false;
    }

    pub fn check_challenge(&mut self, cx: &mut Context<Self>) {
        if self.challenge_pending {
            return;
        }
        let Some(position) = self.model.current_position().cloned() else {
            self.model.set_warning(t("warning.no_step_to_check"));
            cx.notify();
            return;
        };
        if self.model.current_challenge().is_none() {
            self.model.set_warning(t("warning.step_not_challenge"));
            cx.notify();
            return;
        }
        self.challenge_pending = true;
        self.challenge_message = Some(t("challenge.checking"));
        self.challenge_output = None;
        cx.notify();
        let save_dirty = self.challenge_needs_save;

        let session = self.model.session();
        cx.spawn(async move |this, cx| {
            let result = session
                .check_challenge(&position.unit_path, &position.step_id, save_dirty)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.challenge_pending = false;
                match result {
                    Ok(result) => this.apply_challenge_result(result, cx),
                    Err(error) => {
                        this.challenge_needs_save = false;
                        this.challenge_message = Some(error.clone());
                        this.model.set_warning(error);
                        cx.notify();
                    }
                }
                this.maybe_auto_advance(cx);
            });
        })
        .detach();
    }

    pub fn skip_challenge(&mut self, cx: &mut Context<Self>) {
        if self.challenge_pending {
            return;
        }
        let Some(position) = self.model.current_position().cloned() else {
            self.model.set_warning(t("warning.no_step_to_skip"));
            cx.notify();
            return;
        };
        self.challenge_pending = true;
        self.challenge_message = Some(t("challenge.skipping"));
        cx.notify();

        let session = self.model.session();
        cx.spawn(async move |this, cx| {
            let result = session
                .skip_challenge(&position.unit_path, &position.step_id)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.challenge_pending = false;
                match result {
                    Ok(result) => this.apply_challenge_result(result, cx),
                    Err(error) => {
                        this.challenge_message = Some(error.clone());
                        this.model.set_warning(error);
                        cx.notify();
                    }
                }
                this.maybe_auto_advance(cx);
            });
        })
        .detach();
    }

    pub fn advance_step(&mut self, cx: &mut Context<Self>) {
        if self.step_advance_pending {
            return;
        }
        if !self.model.can_advance_step() {
            self.model.set_warning(t("warning.pass_or_skip_first"));
            cx.notify();
            return;
        }
        self.step_advance_pending = true;
        self.stop_narration(cx);
        tracing::debug!(
            target: "cynapse.client.flow",
            position = ?self.model.current_position(),
            "advancing step"
        );
        let session = self.model.session();
        match session.advance_step() {
            Ok(()) => {
                self.challenge_message = None;
                self.challenge_output = None;
                self.challenge_needs_save = false;
                cx.notify();
            }
            Err(error) => {
                self.step_advance_pending = false;
                self.model.set_warning(error);
                cx.notify();
            }
        }
    }

    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if !self.can_retry() {
            return;
        }
        if retry_advance_after_failure(self.model.state().learning().phase, self.at_unit_boundary())
        {
            self.advance_unit(cx);
            return;
        }
        self.stop_narration(cx);
        let session = self.model.session();
        match session.retry_playback() {
            Ok(()) => {
                self.model.clear_warnings();
                cx.notify();
            }
            Err(error) => {
                self.model.set_warning(error);
                cx.notify();
            }
        }
    }

    pub fn advance_unit(&mut self, cx: &mut Context<Self>) {
        let active_mode = self.model.state().learning().mode;
        let mode = active_mode.unwrap_or(LearningMode::ByStep);
        if self.advance_pending || self.reset_pending {
            return;
        }
        let target_unit = self.next_unit().map(|unit| unit.path);
        tracing::debug!(
            target: "cynapse.client.flow",
            ?target_unit,
            ?mode,
            "advancing to next unit"
        );
        if let Some(unit_path) = target_unit.clone() {
            self.model
                .set_pending_learning_intent(Some(PendingLearningIntent { unit_path, mode }));
        }
        self.advance_pending = true;
        self.step_advance_pending = false;
        self.stop_narration(cx);
        cx.notify();
        let session = self.model.session();
        cx.spawn(async move |this, cx| {
            let result = session.advance_unit(mode).await;
            let _ = this.update(cx, |this, cx| {
                this.advance_pending = false;
                this.model.set_pending_learning_intent(None);
                match result {
                    Ok(state) => {
                        let next_unit = state
                            .selected_unit
                            .as_ref()
                            .map(|unit| unit.path.clone())
                            .or_else(|| {
                                state
                                    .active_course
                                    .as_ref()
                                    .and_then(|course| course.current_unit_path.clone())
                            });
                        this.model.apply_course_state(state);
                        if let Some(unit_path) = next_unit {
                            if this.model.state().learning().mode.is_some() {
                                this.model.move_learning_unit(&unit_path);
                            } else if let Err(error) =
                                this.model.start_learning_local(mode, &unit_path)
                            {
                                if this.model.state().learning().mode.is_some() {
                                    this.model.move_learning_unit(&unit_path);
                                } else {
                                    this.model.set_warning(error);
                                }
                            }
                        }
                    }
                    Err(warning) => {
                        apply_advance_failure(&mut this.model, &warning);
                    }
                }
                this.maybe_auto_advance(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn maybe_auto_advance(&mut self, cx: &mut Context<Self>) {
        if self.reset_pending || self.challenge_pending || self.advance_pending {
            return;
        }
        let action = self.next_action();
        if action.prompt {
            if self.model.state().learning().mode == Some(LearningMode::Continuous) {
                tracing::debug!(
                    target: "cynapse.client.flow",
                    "continuous learning stopped before a touched unit"
                );
                self.next_prompt_mode = Some(LearningMode::Continuous);
                self.model.stop_learning_local();
                cx.notify();
            }
            return;
        }
        if action.automatic && self.can_run_next_action(&action) {
            match action.target {
                Some(NextTarget::Step) => {
                    let Some(position) = self.model.current_position() else {
                        return;
                    };
                    let key = (position.unit_path.clone(), position.step_id.clone());
                    if self.last_auto_step.as_ref() != Some(&key) {
                        self.last_auto_step = Some(key);
                        tracing::debug!(
                            target: "cynapse.client.flow",
                            unit_path = %position.unit_path,
                            step_id = %position.step_id,
                            "auto-advancing the next step"
                        );
                        self.advance_step(cx);
                    }
                }
                Some(NextTarget::Unit(target)) => {
                    let Some(position) = self.model.current_position() else {
                        return;
                    };
                    if self.last_auto_unit.as_ref() == Some(&position.unit_path) {
                        return;
                    }
                    self.last_auto_unit = Some(position.unit_path.clone());
                    if self.model.state().learning().mode == Some(LearningMode::Continuous) {
                        self.advance_unit(cx);
                    } else {
                        self.select_unit(target.path, cx);
                    }
                }
                None => {}
            }
        }
    }

    fn apply_challenge_result(&mut self, result: ChallengeCheckResult, cx: &mut Context<Self>) {
        tracing::debug!(
            target: "cynapse.client.flow",
            outcome = ?result.outcome,
            "challenge result"
        );
        self.challenge_needs_save = result.outcome == ChallengeOutcome::WorkspaceDirty;
        self.challenge_message = Some(match result.outcome {
            ChallengeOutcome::Passed => t("challenge.passed"),
            ChallengeOutcome::Failed => result
                .message
                .clone()
                .unwrap_or_else(|| t("challenge.not_passed")),
            ChallengeOutcome::Skipped => t("challenge.skipped"),
            ChallengeOutcome::WorkspaceDirty => t("challenge.workspace_dirty"),
            ChallengeOutcome::Cancelled => result
                .message
                .clone()
                .unwrap_or_else(|| t("challenge.cancelled")),
            ChallengeOutcome::Error => result
                .message
                .clone()
                .unwrap_or_else(|| t("challenge.failed")),
        });
        self.challenge_output = result.output;
        self.challenge_exit_code = result.exit_code;
        self.challenge_output_truncated = result.output_truncated;
        cx.notify();
    }

    pub fn window_title(&self) -> String {
        "Cynapse Client".to_string()
    }

    pub fn caption_visible(&self) -> bool {
        self.caption_visible
    }

    pub fn caption_text(&self) -> &str {
        &self.caption_text
    }

    pub(crate) fn caption_formulas_enabled(&self) -> bool {
        self.caption_formulas_enabled
    }

    pub(crate) fn show_caption_formula_source(&mut self, caption: &str, cx: &mut Context<Self>) {
        if self.caption_text == caption && self.caption_visible && self.caption_formulas_enabled {
            self.caption_formulas_enabled = false;
            cx.emit(ClientIntent::ShowCaption);
            cx.notify();
        }
    }

    pub fn caption_height(&self) -> u32 {
        self.caption_height
    }

    pub fn caption_width(&self) -> u32 {
        self.caption_width
    }

    /// Set the pre-measured window size silently (no re-place intent): this is
    /// called before the caption window is shown, so place_caption uses the
    /// correct size on the very first visible frame.
    pub fn set_caption_size_silent(&mut self, width: f32, height: f32) {
        self.caption_width = width.round().max(1.0) as u32;
        self.caption_height = height.round().max(1.0) as u32;
    }

    pub fn caption_font_size(&self) -> f32 {
        f32::from(self.model.settings().caption.font_size)
    }

    pub fn caption_font_family(&self) -> CaptionFontFamily {
        self.model.settings().caption.font_family
    }

    pub fn caption_text_color(&self) -> &str {
        &self.model.settings().caption.text_color
    }

    pub fn caption_accent_color(&self) -> &str {
        &self.model.settings().caption.accent_color
    }

    pub fn caption_background_color(&self) -> Option<&str> {
        self.model.settings().caption.background_color.as_deref()
    }

    pub fn caption_background_enabled(&self) -> bool {
        self.model.settings().caption.background_enabled
    }

    pub fn caption_background_opacity(&self) -> f32 {
        f32::from(self.model.settings().caption.background_opacity) / 100.0
    }

    pub fn caption_text_opacity(&self) -> f32 {
        f32::from(self.model.settings().caption.text_opacity) / 100.0
    }

    pub fn caption_width_percent(&self) -> u8 {
        self.model.settings().caption.width_percent
    }

    /// The edge the caption is drawn at. Settings holds the default; the mouse
    /// toggles a runtime override that persists across captions.
    pub fn caption_position(&self) -> CaptionPosition {
        let base = self.model.settings().caption.position;
        if self.caption_flipped {
            base.flipped()
        } else {
            base
        }
    }

    pub fn caption_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.caption_rect
    }

    pub fn set_caption_rect(&mut self, rect: Option<(i32, i32, i32, i32)>) {
        self.caption_rect = rect;
    }

    /// Toggle the runtime edge; the persisted setting stays the default.
    pub fn toggle_caption_edge(&mut self, cx: &mut Context<Self>) {
        self.caption_flipped = !self.caption_flipped;
        cx.emit(ClientIntent::CaptionFlipped);
        // CaptionView reads caption_position() to pin the card to the matching
        // edge; re-render so the card follows the window on hover flip.
        cx.notify();
    }

    pub fn settings(&self) -> &ClientSettings {
        self.model.settings()
    }

    pub fn settings_error(&self) -> Option<&str> {
        self.settings_error.as_deref()
    }

    pub fn set_settings_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        if self.settings_error != error {
            self.settings_error = error;
            cx.notify();
        }
    }

    pub fn set_theme(&mut self, theme: ThemePreference, cx: &mut Context<Self>) {
        self.model
            .update_settings(|settings| settings.theme = theme);
        self.emit_settings(true, false, cx);
    }

    pub fn set_language(&mut self, language: LanguagePreference, cx: &mut Context<Self>) {
        self.model
            .update_settings(|settings| settings.language = language);
        self.emit_settings(false, false, cx);
    }

    pub fn set_volume(&mut self, volume: u8, cx: &mut Context<Self>) {
        let volume = volume.min(100);
        if volume == self.settings().volume {
            return;
        }
        self.model
            .update_settings(|settings| settings.volume = volume);
        if let Some(narration) = &self.narration {
            narration.set_volume(volume);
        }
        cx.notify();
        let delay = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(250));
        self.pending_volume_save = Some(cx.spawn(async move |this, cx| {
            delay.await;
            let _ = this.update(cx, |this, cx| this.emit_settings(false, false, cx));
        }));
    }

    pub fn take_pending_volume_settings(&mut self) -> Option<ClientSettings> {
        self.pending_volume_save
            .take()
            .map(|_| self.settings().clone())
    }

    pub fn set_course_store_dir(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        match path {
            Some(path) if path.is_absolute() => {
                self.model
                    .update_settings(|settings| settings.course_store_dir = Some(path));
            }
            Some(_) => return,
            None => self
                .model
                .update_settings(|settings| settings.course_store_dir = None),
        }
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_position(&mut self, position: CaptionPosition, cx: &mut Context<Self>) {
        self.model
            .update_settings(|settings| settings.caption.position = position);
        self.emit_settings(false, true, cx);
    }

    pub fn set_caption_font_family(
        &mut self,
        font_family: CaptionFontFamily,
        cx: &mut Context<Self>,
    ) {
        self.model
            .update_settings(|settings| settings.caption.font_family = font_family);
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_text_color(&mut self, color: String, cx: &mut Context<Self>) {
        self.model
            .update_settings(|settings| settings.caption.text_color = color);
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_accent_color(&mut self, color: String, cx: &mut Context<Self>) {
        self.model
            .update_settings(|settings| settings.caption.accent_color = color);
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_background_color(&mut self, color: String, cx: &mut Context<Self>) {
        self.model
            .update_settings(|settings| settings.caption.background_color = Some(color));
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_background_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.model
            .update_settings(|settings| settings.caption.set_background_enabled(enabled));
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_font_size(&mut self, font_size: u16, cx: &mut Context<Self>) {
        self.model.update_settings(|settings| {
            settings.caption.font_size = font_size.clamp(20, 48);
        });
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_opacity(&mut self, opacity: u8, cx: &mut Context<Self>) {
        self.model.update_settings(|settings| {
            settings.caption.background_opacity = opacity.min(100);
        });
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_text_opacity(&mut self, opacity: u8, cx: &mut Context<Self>) {
        self.model.update_settings(|settings| {
            settings.caption.text_opacity = opacity.min(100);
        });
        self.emit_settings(false, false, cx);
    }

    pub fn set_caption_width(&mut self, width: u8, cx: &mut Context<Self>) {
        self.model.update_settings(|settings| {
            settings.caption.width_percent = width.clamp(50, 100);
        });
        self.emit_settings(false, true, cx);
    }

    fn emit_settings(
        &mut self,
        theme_changed: bool,
        caption_geometry_changed: bool,
        cx: &mut Context<Self>,
    ) {
        // Every settings save includes the latest volume.
        self.pending_volume_save = None;
        let settings = self.model.settings().clone();
        cx.emit(ClientIntent::SettingsChanged {
            settings,
            theme_changed,
            caption_geometry_changed,
        });
        cx.notify();
    }

    fn handle_playback(&mut self, playback: &PlaybackState, cx: &mut Context<Self>) {
        let step_key = playback_step_key(playback);
        let step_changed = step_key != self.last_step_key;
        if self.step_advance_pending
            && (step_changed || !matches!(playback, PlaybackState::StepComplete { .. }))
        {
            self.step_advance_pending = false;
        }
        if step_changed {
            self.last_step_key = step_key;
            self.challenge_pending = false;
            self.challenge_message = None;
            self.challenge_output = None;
            self.challenge_needs_save = false;
        }

        let current = match playback {
            PlaybackState::Narrating { clip, position } => Some((clip, position)),
            PlaybackState::StepComplete {
                continuation: Some(continuation),
                position,
                ..
            } => Some((&continuation.clip, position)),
            _ => None,
        };

        if let Some((clip, position)) = current {
            if self.active_clip.as_ref() == Some(clip) {
                return;
            }
            self.active_clip = Some(clip.clone());
            let session = self.model.session();
            match session.narration_audio(position, clip) {
                Ok(Some(asset)) => {
                    self.set_caption(asset.caption.clone(), cx);
                    if let Some(path) = asset.audio_path {
                        if let Some(narration) = &self.narration {
                            narration.play(
                                clip.clone(),
                                PathBuf::from(path),
                                asset.offset_ms.unwrap_or(0),
                                asset.duration_ms,
                            );
                        } else {
                            let _ = session
                                .narration_failed(clip.clone(), t("error.audio_host_unavailable"));
                        }
                    } else if let Some(narration) = &self.narration {
                        narration.play_caption(clip.clone(), asset.duration_ms);
                    } else {
                        let _ = session
                            .narration_failed(clip.clone(), t("error.caption_host_unavailable"));
                    }
                }
                Ok(None) => {
                    let _ = session
                        .narration_failed(clip.clone(), t("error.narration_asset_unavailable"));
                    self.active_clip = None;
                    self.set_caption(None, cx);
                }
                Err(error) => {
                    let _ = session.narration_failed(clip.clone(), error);
                    self.active_clip = None;
                    self.set_caption(None, cx);
                }
            }
        } else if should_stop_narration(playback, self.active_clip.is_some(), step_changed) {
            self.stop_narration(cx);
        }
    }

    fn stop_narration(&mut self, cx: &mut Context<Self>) {
        if let Some(narration) = &self.narration {
            narration.stop();
        }
        self.active_clip = None;
        self.set_caption(None, cx);
    }

    fn set_caption(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        let text = text.unwrap_or_default();
        let visible = !text.is_empty();
        if self.caption_text == text && self.caption_visible == visible {
            return;
        }
        self.caption_text = text;
        self.caption_formulas_enabled = true;
        self.caption_visible = visible;
        if !visible {
            self.caption_rect = None;
        }
        // Keep the last measured height. Resetting to 0 makes caption_rect jump
        // to a full-screen window for one frame, which is visible as a flash.
        // CaptionView re-measures on the text change and adjusts from there.
        cx.emit(if visible {
            ClientIntent::ShowCaption
        } else {
            ClientIntent::HideCaption
        });
        cx.notify();
    }
}

fn adapter_status_for(adapters: &[AdapterId], connection: &ConnectionState) -> AdapterStatus {
    let connected = match connection {
        ConnectionState::Connected { adapter } => Some(adapter.as_str()),
        _ => None,
    };
    if let Some(required) = adapters.first() {
        let connected = connected == Some(required.connection_name());
        return AdapterStatus {
            tooltip: if connected {
                t("adapter.connected")
            } else {
                t("adapter.waiting")
            },
            connected,
            required: true,
        };
    }
    AdapterStatus {
        tooltip: if connected.is_some() {
            t("adapter.connected")
        } else {
            t("adapter.disconnected")
        },
        connected: connected.is_some(),
        required: false,
    }
}

fn format_download_size(downloaded: u64, total: Option<u64>) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let downloaded = downloaded as f64 / MIB;
    match total.filter(|total| *total > 0) {
        Some(total) => format!("{downloaded:.1} / {:.1} MB", total as f64 / MIB),
        None => format!("{downloaded:.1} MB"),
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn transition_kind_label(kind: ClientTransitionKind) -> String {
    match kind {
        ClientTransitionKind::Preparing => t("transition.preparing"),
        ClientTransitionKind::Selecting => t("transition.selecting"),
        ClientTransitionKind::Starting => t("transition.starting"),
        ClientTransitionKind::Stopping => t("transition.stopping"),
        ClientTransitionKind::Resetting => t("transition.resetting"),
        ClientTransitionKind::Opening => t("transition.opening"),
        ClientTransitionKind::Advancing => t("transition.advancing"),
        ClientTransitionKind::Retrying => t("transition.retrying"),
    }
}

/// Whether a playback-state change ends the active narration tail. A finished
/// Step keeps its tail while the clip is still running; advancing to another
/// Step during `Playing` stops the old tail.
fn should_stop_narration(
    playback: &PlaybackState,
    has_live_media: bool,
    step_changed: bool,
) -> bool {
    match playback {
        PlaybackState::Playing { .. } => step_changed,
        PlaybackState::StepComplete {
            continuation: None, ..
        } => !has_live_media,
        PlaybackState::Narrating { .. }
        | PlaybackState::StepComplete {
            continuation: Some(_),
            ..
        }
        | PlaybackState::UnitComplete { .. } => false,
        PlaybackState::Idle
        | PlaybackState::Ready { .. }
        | PlaybackState::Paused { .. }
        | PlaybackState::Error { .. } => true,
    }
}

fn playback_step_key(playback: &PlaybackState) -> Option<(String, String)> {
    let position = match playback {
        PlaybackState::Playing { position, .. }
        | PlaybackState::Narrating { position, .. }
        | PlaybackState::Paused { position, .. }
        | PlaybackState::StepComplete { position, .. }
        | PlaybackState::UnitComplete { position } => position,
        PlaybackState::Ready { position } => position.as_ref()?,
        PlaybackState::Error { position, .. } => position.as_ref()?,
        PlaybackState::Idle => return None,
    };
    Some((position.unit_path.clone(), position.step_id.clone()))
}

#[cfg(test)]
mod tests;
