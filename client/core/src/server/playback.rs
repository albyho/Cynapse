use super::*;
use crate::course::{ActionNarration, ActionNarrationPhase};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STEP_RUN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub(super) struct ActiveAction {
    pub(super) cursor: PlaybackCursor,
    pub(super) id: String,
}

pub(super) struct ActionDispatchContext<'a> {
    pub(super) index: &'a PlaybackIndex,
    pub(super) active_action: &'a mut Option<ActiveAction>,
    pub(super) local_wait: &'a mut Option<LocalWait>,
    pub(super) skip_waits: bool,
    pub(super) skip_waits_through: Option<PlaybackCursor>,
    pub(super) skip_unit_final_wait: bool,
    pub(super) action_narration: &'a mut ActionNarrationRuntime,
    pub(super) playback: &'a mut PlaybackState,
    pub(super) playback_tx: &'a watch::Sender<PlaybackState>,
    pub(super) correlation_id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(super) struct StartedActionNarration {
    pub(super) cursor: PlaybackCursor,
    pub(super) clip: NarrationClipRef,
}

#[derive(Debug, Clone)]
pub(super) enum ActionNarrationEnd {
    BeforeNext(StartedActionNarration),
    BeforeEnded(Option<StartedActionNarration>),
    DuringNext(StartedActionNarration),
    DuringBeforeAction,
    DuringAfterAction,
    AfterNext(StartedActionNarration),
    AfterComplete,
}

#[derive(Debug, Clone)]
pub(super) enum ActionNarrationDone {
    Complete,
    WaitForDuringStart,
    WaitForDuring(StartedActionNarration),
    StartAfter(StartedActionNarration),
}

#[derive(Debug, Clone)]
pub(super) struct ActiveActionNarration {
    pub(super) clip: NarrationClipRef,
    started: bool,
}

#[derive(Debug)]
struct ActiveActionBeat {
    cursor: PlaybackCursor,
    step_run_id: u64,
    narration: ActionNarration,
    active: Option<ActiveActionNarration>,
    action_done: bool,
}

#[derive(Debug, Default)]
pub(super) struct ActionNarrationRuntime {
    active: Option<ActiveActionBeat>,
    step_run: Option<(usize, u64)>,
}

impl ActionNarrationRuntime {
    pub(super) fn cursor(&self) -> Option<PlaybackCursor> {
        self.active.as_ref().map(|beat| beat.cursor)
    }

    pub(super) fn active_clip(&self) -> Option<&ActiveActionNarration> {
        self.active.as_ref().and_then(|beat| beat.active.as_ref())
    }

    pub(super) fn action_is_done(&self) -> bool {
        self.active.as_ref().is_some_and(|beat| beat.action_done)
    }

    pub(super) fn matches(&self, clip: &NarrationClipRef) -> bool {
        self.active_clip()
            .is_some_and(|active| active.clip == *clip)
    }

    pub(super) fn mark_started(&mut self, clip: &NarrationClipRef) -> Option<ActionNarrationPhase> {
        let active = self.active.as_mut()?.active.as_mut()?;
        if active.clip != *clip {
            return None;
        }
        if active.started {
            return None;
        }
        active.started = true;
        Some(active.clip.phase)
    }

    fn begin(
        &mut self,
        cursor: PlaybackCursor,
        narration: ActionNarration,
    ) -> Option<StartedActionNarration> {
        let step_run_id = match self.step_run {
            Some((step_index, step_run_id)) if step_index == cursor.step_index => step_run_id,
            _ => {
                let step_run_id = NEXT_STEP_RUN_ID.fetch_add(1, Ordering::Relaxed);
                self.step_run = Some((cursor.step_index, step_run_id));
                step_run_id
            }
        };
        let mut beat = ActiveActionBeat {
            cursor,
            step_run_id,
            narration,
            active: None,
            action_done: false,
        };
        let started = if beat.narration.before.is_some() {
            start_phase(&mut beat, ActionNarrationPhase::Before)
        } else if beat.narration.during.is_some() {
            start_phase(&mut beat, ActionNarrationPhase::During)
        } else {
            None
        };
        self.active = Some(beat);
        started
    }

    pub(super) fn end_active(&mut self, clip: &NarrationClipRef) -> Option<ActionNarrationEnd> {
        let beat = self.active.as_mut()?;
        let active = beat.active.as_ref()?;
        if active.clip != *clip || !active.started {
            return None;
        }
        let phase = active.clip.phase;
        let clip_index = active.clip.clip_index;
        beat.active = None;
        match phase {
            ActionNarrationPhase::Before => {
                if let Some(next) = start_next_clip(beat, phase, clip_index + 1) {
                    return Some(ActionNarrationEnd::BeforeNext(next));
                }
                let during = if beat.narration.during.is_some() {
                    start_phase(beat, ActionNarrationPhase::During)
                } else {
                    None
                };
                Some(ActionNarrationEnd::BeforeEnded(during))
            }
            ActionNarrationPhase::During => {
                if let Some(next) = start_next_clip(beat, phase, clip_index + 1) {
                    return Some(ActionNarrationEnd::DuringNext(next));
                }
                if beat.action_done {
                    Some(ActionNarrationEnd::DuringAfterAction)
                } else {
                    Some(ActionNarrationEnd::DuringBeforeAction)
                }
            }
            ActionNarrationPhase::After => {
                if let Some(next) = start_next_clip(beat, phase, clip_index + 1) {
                    return Some(ActionNarrationEnd::AfterNext(next));
                }
                Some(ActionNarrationEnd::AfterComplete)
            }
        }
    }

    pub(super) fn action_done(&mut self, cursor: PlaybackCursor) -> Option<ActionNarrationDone> {
        let beat = self.active.as_mut()?;
        if beat.cursor != cursor {
            return None;
        }
        beat.action_done = true;
        if let Some(active) = beat.active.as_ref()
            && active.clip.phase == ActionNarrationPhase::During
        {
            if !active.started {
                return Some(ActionNarrationDone::WaitForDuringStart);
            }
            return Some(ActionNarrationDone::WaitForDuring(started_from_active(
                cursor, active,
            )));
        }
        if beat.narration.after.is_some() {
            let started = start_phase(beat, ActionNarrationPhase::After)
                .expect("configured After Clip starts");
            return Some(ActionNarrationDone::StartAfter(started));
        }
        self.active = None;
        Some(ActionNarrationDone::Complete)
    }

    pub(super) fn complete_active_beat(&mut self, cursor: PlaybackCursor) {
        if self.cursor() == Some(cursor) {
            self.active = None;
        }
    }

    pub(super) fn clear(&mut self) {
        self.active = None;
        self.step_run = None;
    }
}

fn start_phase(
    beat: &mut ActiveActionBeat,
    phase: ActionNarrationPhase,
) -> Option<StartedActionNarration> {
    beat.narration.clips(phase)?;
    start_clip(beat, phase, 0)
}

fn start_next_clip(
    beat: &mut ActiveActionBeat,
    phase: ActionNarrationPhase,
    clip_index: usize,
) -> Option<StartedActionNarration> {
    let clips = beat.narration.clips(phase)?;
    if clip_index >= clips.len() {
        return None;
    }
    start_clip(beat, phase, clip_index)
}

fn start_clip(
    beat: &mut ActiveActionBeat,
    phase: ActionNarrationPhase,
    clip_index: usize,
) -> Option<StartedActionNarration> {
    let clip = NarrationClipRef {
        step_run_id: beat.step_run_id,
        action_index: beat.cursor.action_index,
        phase,
        clip_index,
    };
    beat.active = Some(ActiveActionNarration {
        clip: clip.clone(),
        started: false,
    });
    Some(StartedActionNarration {
        cursor: beat.cursor,
        clip,
    })
}

fn started_from_active(
    cursor: PlaybackCursor,
    active: &ActiveActionNarration,
) -> StartedActionNarration {
    StartedActionNarration {
        cursor,
        clip: active.clip.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlaybackCursor {
    pub(super) step_index: usize,
    pub(super) action_index: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PlaybackIndex {
    parent_index: Vec<usize>,
    parent_total: Vec<usize>,
    unit_index: Vec<usize>,
    unit_total: Vec<usize>,
    action_prefix: Vec<usize>,
}

impl PlaybackIndex {
    pub(super) fn new(steps: &[PlaybackStep]) -> Self {
        let mut parent_counts = HashMap::<&str, usize>::new();
        let mut unit_counts = HashMap::<&str, usize>::new();
        for step in steps {
            *parent_counts.entry(step.parent_id.as_str()).or_default() += 1;
            *unit_counts.entry(step.unit_path.as_str()).or_default() += 1;
        }
        let mut parent_seen = HashMap::<&str, usize>::new();
        let mut unit_seen = HashMap::<&str, usize>::new();
        let mut parent_index = Vec::with_capacity(steps.len());
        let mut parent_total = Vec::with_capacity(steps.len());
        let mut unit_index = Vec::with_capacity(steps.len());
        let mut unit_total = Vec::with_capacity(steps.len());
        let mut action_prefix = Vec::with_capacity(steps.len() + 1);
        action_prefix.push(0);
        for step in steps {
            let parent_position = parent_seen.entry(step.parent_id.as_str()).or_default();
            let unit_position = unit_seen.entry(step.unit_path.as_str()).or_default();
            parent_index.push(*parent_position);
            parent_total.push(parent_counts[step.parent_id.as_str()]);
            unit_index.push(*unit_position);
            unit_total.push(unit_counts[step.unit_path.as_str()]);
            *parent_position += 1;
            *unit_position += 1;
            action_prefix
                .push(action_prefix.last().copied().unwrap_or_default() + step.actions.len());
        }
        Self {
            parent_index,
            parent_total,
            unit_index,
            unit_total,
            action_prefix,
        }
    }

    pub(super) fn total_actions(&self) -> usize {
        self.action_prefix.last().copied().unwrap_or_default()
    }
}

pub(super) async fn send_action(
    socket: &mut WebSocketStream<TcpStream>,
    steps: &[PlaybackStep],
    cursor: PlaybackCursor,
    context: ActionDispatchContext<'_>,
) -> Result<bool, ()> {
    let ActionDispatchContext {
        index,
        active_action,
        local_wait,
        skip_waits,
        skip_waits_through,
        skip_unit_final_wait,
        action_narration,
        playback,
        playback_tx,
        correlation_id,
    } = context;
    let action = steps
        .get(cursor.step_index)
        .and_then(|step| step.actions.get(cursor.action_index))
        .ok_or(())?;
    // A skipped Wait is a pure timing pause: replay and `skip_waits` must
    // complete it immediately without starting its audio or caption.
    if action.narration.has_any()
        && !wait_is_skipped(
            action,
            steps,
            cursor,
            skip_waits,
            skip_waits_through,
            skip_unit_final_wait,
        )
    {
        if action_narration.cursor() == Some(cursor) {
            return Ok(true);
        }
        if let Some(started) = action_narration.begin(cursor, action.narration.clone()) {
            publish_action_narration(
                steps,
                index,
                playback,
                playback_tx,
                &started,
                correlation_id,
            )?;
            return Ok(true);
        }
    }
    dispatch_action(
        socket,
        steps,
        index,
        cursor,
        active_action,
        local_wait,
        skip_waits,
        skip_waits_through,
        skip_unit_final_wait,
        playback,
        playback_tx,
        correlation_id,
    )
    .await
}

/// Whether this `Wait` must complete immediately instead of playing its timer
/// and narration: forced skip, or the final `Wait` of a Unit in by-step learning.
fn wait_is_skipped(
    action: &PlaybackAction,
    steps: &[PlaybackStep],
    cursor: PlaybackCursor,
    skip_waits: bool,
    skip_waits_through: Option<PlaybackCursor>,
    skip_unit_final_wait: bool,
) -> bool {
    matches!(&action.action, Action::Wait(_))
        && (skip_waits
            || skip_waits_through.is_some_and(|through| {
                cursor.step_index == through.step_index
                    && cursor.action_index <= through.action_index
            })
            || (skip_unit_final_wait && next_cursor_in_unit(steps, cursor).is_none()))
}

pub(super) fn publish_action_narration(
    steps: &[PlaybackStep],
    index: &PlaybackIndex,
    playback: &mut PlaybackState,
    playback_tx: &watch::Sender<PlaybackState>,
    started: &StartedActionNarration,
    correlation_id: Option<&str>,
) -> Result<(), ()> {
    let position = playback_position(steps, index, started.cursor).ok_or(())?;
    *playback = PlaybackState::Narrating {
        clip: started.clip.clone(),
        position: position.clone(),
    };
    let _ = playback_tx.send(playback.clone());
    log::info!(
        target: "cynapse.server",
        "action.narration.started correlation_id={} phase={:?} step_id={} action_index={} step_run_id={} clip_index={}",
        correlation_id.unwrap_or("-"),
        started.clip.phase,
        position.step_id,
        started.cursor.action_index,
        started.clip.step_run_id,
        started.clip.clip_index
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_action(
    socket: &mut WebSocketStream<TcpStream>,
    steps: &[PlaybackStep],
    index: &PlaybackIndex,
    cursor: PlaybackCursor,
    active_action: &mut Option<ActiveAction>,
    local_wait: &mut Option<LocalWait>,
    skip_waits: bool,
    skip_waits_through: Option<PlaybackCursor>,
    skip_unit_final_wait: bool,
    playback: &mut PlaybackState,
    playback_tx: &watch::Sender<PlaybackState>,
    correlation_id: Option<&str>,
) -> Result<bool, ()> {
    let step = steps.get(cursor.step_index).ok_or(())?;
    let action = step.actions.get(cursor.action_index).ok_or(())?;
    let payload = serde_json::to_value(&action.action).map_err(|_| ())?;
    let mut envelope = Envelope::new(MessageType::Action, payload);
    envelope.unit_path = Some(step.unit_path.clone());
    let action_id = envelope.id.clone();
    let payload = &envelope.payload;
    log::info!(
        target: "cynapse.server",
        "action.dispatch correlation_id={} action_id={} step_id={} unit_path={} action_index={} type={} path={} cell_id={} from_cell_id={} to_cell_id={} before_cell_id={} after_cell_id={} new_cell_id={} session_key={} cwd={}",
        correlation_id.unwrap_or("-"),
        action_id,
        step.step_id,
        step.unit_path,
        cursor.action_index,
        payload.get("type").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("path").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("cell_id").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("from_cell_id").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("to_cell_id").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("before_cell_id").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("after_cell_id").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("new_cell_id").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("session_key").and_then(serde_json::Value::as_str).unwrap_or("-"),
        payload.get("cwd").and_then(serde_json::Value::as_str).unwrap_or("-")
    );
    if let Action::Wait(_) = &action.action {
        let delay = if wait_is_skipped(
            action,
            steps,
            cursor,
            skip_waits,
            skip_waits_through,
            skip_unit_final_wait,
        ) {
            Duration::ZERO
        } else {
            Duration::from_millis(action.effective_wait_duration_ms().ok_or(())?)
        };
        *local_wait = Some(LocalWait {
            cursor,
            deadline: tokio::time::Instant::now().checked_add(delay).ok_or(())?,
        });
    } else {
        let json = envelope.to_json().map_err(|_| ())?;
        if socket.send(Message::Text(json.into())).await.is_err() {
            return Ok(false);
        }
        *local_wait = None;
    }

    *active_action = Some(ActiveAction {
        cursor,
        id: action_id.clone(),
    });
    *playback = PlaybackState::Playing {
        action_id: action_id.clone(),
        action_label: action_progress_label(action),
        position: playback_position(steps, index, cursor).ok_or(())?,
    };
    if let PlaybackState::Playing {
        action_label,
        position,
        ..
    } = playback
    {
        log::info!(
            target: "cynapse.server",
            "action.started correlation_id={} action_id={} step_id={} unit_path={} action_index={} action={}",
            correlation_id.unwrap_or("-"),
            action_id,
            position.step_id,
            position.unit_path,
            position.action_index,
            action_label
        );
    }
    let _ = playback_tx.send(playback.clone());
    Ok(true)
}

pub(super) fn log_action_completed(
    steps: &[PlaybackStep],
    cursor: PlaybackCursor,
    correlation_id: Option<&str>,
    next: &str,
) {
    let step = steps.get(cursor.step_index);
    log::info!(
        target: "cynapse.server",
        "action.completed correlation_id={} step_id={} unit_path={} action_index={} next={}",
        correlation_id.unwrap_or("-"),
        step.map(|step| step.step_id.as_str()).unwrap_or("-"),
        step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
        cursor.action_index,
        next
    );
}

fn action_progress_label(action: &PlaybackAction) -> String {
    use cynapse_client_i18n::{t, t_with};

    match &action.action {
        Action::Present => t("action.present"),
        Action::Wait(wait) => {
            let ms = action
                .effective_wait_duration_ms()
                .unwrap_or(wait.duration_ms)
                .to_string();
            t_with("action.wait", &[("ms", &ms)])
        }
        Action::OpenFile(action) => t_with("action.open_file", &[("path", &action.path)]),
        Action::OpenPreview(action) => t_with("action.open_preview", &[("path", &action.path)]),
        Action::OpenFileWithPreview(action) => {
            t_with("action.open_file_and_preview", &[("path", &action.path)])
        }
        Action::CloseFile(action) => t_with("action.close_file", &[("path", &action.path)]),
        Action::ClosePreview(action) => t_with("action.close_preview", &[("path", &action.path)]),
        Action::RevealText(action) => t_with("action.reveal_text", &[("path", &action.path)]),
        Action::ClearReveal => t("action.clear_reveal"),
        Action::InsertText(action) => t_with("action.insert_text", &[("path", &action.path)]),
        Action::DeleteText(action) => t_with("action.delete_text", &[("path", &action.path)]),
        Action::ReplaceText(action) => t_with("action.replace_text", &[("path", &action.path)]),
        Action::CreateDirectory(action) => {
            t_with("action.create_directory", &[("path", &action.path)])
        }
        Action::CreateFile(action) => t_with("action.create_file", &[("path", &action.path)]),
        Action::DeleteFile(action) => t_with("action.delete_file", &[("path", &action.path)]),
        Action::DeleteDirectory(action) => {
            t_with("action.delete_directory", &[("path", &action.path)])
        }
        Action::TerminalEnsure(action) => t_with(
            "action.terminal_ensure",
            &[("session", &action.session_key)],
        ),
        Action::TerminalCommand(action) => {
            t_with("action.terminal_command", &[("command", &action.command)])
        }
        Action::JupyterLabOpenNotebook(action) => {
            t_with("action.open_notebook", &[("path", &action.path)])
        }
        Action::JupyterLabCellSelect(action) => {
            t_with("action.select_cell", &[("cell_id", &action.cell_id)])
        }
        Action::JupyterLabCellRun(action) => {
            match (action.from_cell_id.as_deref(), action.to_cell_id.as_deref()) {
                (Some(from_cell_id), Some(to_cell_id)) if from_cell_id == to_cell_id => {
                    t_with("action.run_cell", &[("cell_id", to_cell_id)])
                }
                (Some(from_cell_id), Some(to_cell_id)) => t_with(
                    "action.run_cell_range",
                    &[("from_cell_id", from_cell_id), ("to_cell_id", to_cell_id)],
                ),
                (Some(from_cell_id), None) => {
                    t_with("action.run_cells_from", &[("from_cell_id", from_cell_id)])
                }
                (None, Some(to_cell_id)) => {
                    t_with("action.run_cells_to", &[("to_cell_id", to_cell_id)])
                }
                (None, None) => t("action.run_all_cells"),
            }
        }
        Action::JupyterLabRestartKernel(action) => {
            t_with("action.restart_kernel", &[("path", &action.path)])
        }
        Action::JupyterLabInterruptKernel(action) => {
            t_with("action.interrupt_kernel", &[("path", &action.path)])
        }
        Action::JupyterLabOpenPreview(action) => {
            t_with("action.open_preview", &[("path", &action.path)])
        }
        Action::JupyterLabCloseDocument(action) => {
            t_with("action.close_document", &[("path", &action.path)])
        }
        Action::JupyterLabCellInsertText(action) => t_with(
            "action.jupyter_lab_cell_insert_text",
            &[("cell_id", &action.cell_id)],
        ),
        Action::JupyterLabCellDeleteText(action) => t_with(
            "action.jupyter_lab_cell_delete_text",
            &[("cell_id", &action.cell_id)],
        ),
        Action::JupyterLabCellReplaceText(action) => t_with(
            "action.jupyter_lab_cell_replace_text",
            &[("cell_id", &action.cell_id)],
        ),
        Action::JupyterLabCellInsert(action) => {
            t_with("action.insert_cell", &[("cell_id", &action.cell_id)])
        }
        Action::JupyterLabCellMove(action) => {
            t_with("action.move_cell", &[("cell_id", &action.cell_id)])
        }
        Action::JupyterLabCellDuplicate(action) => {
            t_with("action.duplicate_cell", &[("cell_id", &action.cell_id)])
        }
        Action::JupyterLabCellDelete(action) => {
            t_with("action.delete_cell", &[("cell_id", &action.cell_id)])
        }
    }
}

pub(super) fn first_cursor(steps: &[PlaybackStep]) -> Option<PlaybackCursor> {
    steps
        .iter()
        .enumerate()
        .find(|(_, step)| !step.actions.is_empty())
        .map(|(step_index, _)| PlaybackCursor {
            step_index,
            action_index: 0,
        })
}

pub(super) fn resume_cursor(
    steps: &[PlaybackStep],
    resume: &PlaybackResume,
) -> Option<PlaybackCursor> {
    let step_index = steps
        .iter()
        .position(|step| step.unit_path == resume.unit_path && step.step_id == resume.step_id)?;
    let step = steps.get(step_index)?;
    if step.actions.is_empty() {
        return None;
    }
    Some(PlaybackCursor {
        step_index,
        action_index: 0,
    })
}

pub(super) fn next_cursor_in_unit(
    steps: &[PlaybackStep],
    cursor: PlaybackCursor,
) -> Option<PlaybackCursor> {
    let current = steps.get(cursor.step_index)?;
    if cursor.action_index + 1 < current.actions.len() {
        return Some(PlaybackCursor {
            step_index: cursor.step_index,
            action_index: cursor.action_index + 1,
        });
    }

    steps
        .iter()
        .enumerate()
        .skip(cursor.step_index + 1)
        .find(|(_, step)| step.unit_path == current.unit_path && !step.actions.is_empty())
        .map(|(step_index, _)| PlaybackCursor {
            step_index,
            action_index: 0,
        })
}

pub(super) fn completed_cursor(steps: &[PlaybackStep], cursor: PlaybackCursor) -> PlaybackCursor {
    PlaybackCursor {
        step_index: cursor.step_index,
        action_index: steps
            .get(cursor.step_index)
            .map_or(0, |step| step.actions.len()),
    }
}

pub(super) fn restart_step_cursor(cursor: PlaybackCursor) -> PlaybackCursor {
    PlaybackCursor {
        step_index: cursor.step_index,
        action_index: 0,
    }
}

pub(super) fn playback_position(
    steps: &[PlaybackStep],
    index: &PlaybackIndex,
    cursor: PlaybackCursor,
) -> Option<PlaybackPosition> {
    let step = steps.get(cursor.step_index)?;
    let overall_action_index = *index.action_prefix.get(cursor.step_index)? + cursor.action_index;

    Some(PlaybackPosition {
        parent_id: step.parent_id.clone(),
        parent_title: step.parent_title.clone(),
        unit_path: step.unit_path.clone(),
        unit_title: step.unit_title.clone(),
        step_id: step.step_id.clone(),
        step_title: step.step_title.clone(),
        step_started: false,
        step_index: *index.parent_index.get(cursor.step_index)?,
        step_total: *index.parent_total.get(cursor.step_index)?,
        unit_step_index: *index.unit_index.get(cursor.step_index)?,
        unit_step_total: *index.unit_total.get(cursor.step_index)?,
        action_index: cursor.action_index,
        action_total: step.actions.len(),
        overall_action_index,
        overall_action_total: index.total_actions(),
    })
}

#[cfg(test)]
mod index_tests {
    use super::*;

    fn step(parent: &str, unit: &str, actions: usize) -> PlaybackStep {
        let step_id = format!("{parent}-{unit}-{actions}");
        PlaybackStep {
            parent_id: parent.to_string(),
            parent_title: parent.to_string(),
            unit_title: unit.to_string(),
            unit_path: unit.to_string(),
            step_id: step_id.clone(),
            step_title: String::new(),
            requires_human: false,
            challenge: false,
            actions: (0..actions)
                .map(|_| crate::course::PlaybackAction {
                    action: crate::course::Action::Wait(crate::course::WaitAction {
                        duration_ms: 1,
                    }),
                    step_id: step_id.clone(),
                    narration: crate::course::ActionNarration::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn resume_wait_skip_stops_after_the_saved_action() {
        let steps = vec![step("chapter-a", "unit-a", 3)];
        let boundary = PlaybackCursor {
            step_index: 0,
            action_index: 1,
        };
        let action = &steps[0].actions[1];
        assert!(wait_is_skipped(
            action,
            &steps,
            PlaybackCursor {
                step_index: 0,
                action_index: 1,
            },
            false,
            Some(boundary),
            false,
        ));
        assert!(!wait_is_skipped(
            action,
            &steps,
            PlaybackCursor {
                step_index: 0,
                action_index: 2,
            },
            false,
            Some(boundary),
            false,
        ));
    }

    #[test]
    fn resume_cursor_replays_from_the_step_start() {
        let steps = vec![step("chapter-a", "unit-a", 3)];
        let resume = PlaybackResume {
            unit_path: "unit-a".to_string(),
            step_id: steps[0].step_id.clone(),
            step_started: true,
            action_index: Some(1),
            recovery: None,
        };
        let cursor = resume_cursor(&steps, &resume).expect("resume cursor");
        assert_eq!(cursor.step_index, 0);
        assert_eq!(cursor.action_index, 0);
    }

    #[test]
    fn position_uses_precomputed_parent_unit_and_action_indexes() {
        let steps = vec![
            step("chapter-a", "unit-a", 2),
            step("chapter-b", "unit-b", 3),
            step("chapter-a", "unit-a", 1),
        ];
        let index = PlaybackIndex::new(&steps);
        let position = playback_position(
            &steps,
            &index,
            PlaybackCursor {
                step_index: 2,
                action_index: 1,
            },
        )
        .expect("position");

        assert_eq!(position.step_index, 1);
        assert_eq!(position.step_total, 2);
        assert_eq!(position.unit_step_index, 1);
        assert_eq!(position.unit_step_total, 2);
        assert_eq!(position.overall_action_index, 6);
        assert_eq!(position.overall_action_total, 6);
    }
}
