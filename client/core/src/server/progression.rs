use super::*;

pub(super) fn current_playback_position(state: &PlaybackState) -> Option<PlaybackPosition> {
    match state {
        PlaybackState::Playing { position, .. }
        | PlaybackState::Narrating { position, .. }
        | PlaybackState::Paused { position, .. }
        | PlaybackState::StepComplete { position, .. }
        | PlaybackState::UnitComplete { position } => Some(position.clone()),
        PlaybackState::Ready { position } => position.clone(),
        PlaybackState::Error { position, .. } => position.clone(),
        PlaybackState::Idle => None,
    }
}

pub(super) struct ActionAdvanceContext<'a> {
    pub(super) socket: &'a mut WebSocketStream<TcpStream>,
    pub(super) steps: &'a [PlaybackStep],
    pub(super) index: &'a PlaybackIndex,
    pub(super) playback_session: &'a mut PlaybackSession,
    pub(super) action_narration: &'a mut ActionNarrationRuntime,
    pub(super) active_action: &'a mut Option<ActiveAction>,
    pub(super) local_wait: &'a mut Option<LocalWait>,
    pub(super) playback_tx: &'a watch::Sender<PlaybackState>,
}

pub(super) enum StepTailCompletion {
    NotApplicable,
    Completed { boundary: Option<PendingBoundary> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionAdvance {
    Dispatch(PlaybackCursor),
    Boundary(PendingBoundary),
}

/// Pure decision for the next cursor after a successful Action.
pub(super) fn plan_after_successful_action(
    steps: &[PlaybackStep],
    cursor: PlaybackCursor,
) -> ActionAdvance {
    let completed = completed_cursor(steps, cursor);
    match next_cursor_in_unit(steps, cursor) {
        Some(next) if next.step_index == cursor.step_index => ActionAdvance::Dispatch(next),
        Some(next) => {
            let next_unit_path = steps
                .get(next.step_index)
                .map(|step| step.unit_path.as_str());
            let current_unit_path = steps
                .get(cursor.step_index)
                .map(|step| step.unit_path.as_str());
            ActionAdvance::Boundary(if next_unit_path == current_unit_path {
                PendingBoundary::Step { completed, next }
            } else {
                PendingBoundary::Unit { completed }
            })
        }
        None => ActionAdvance::Boundary(PendingBoundary::Unit { completed }),
    }
}

pub(super) fn publish_step_completion_with_action_narration(
    steps: &[PlaybackStep],
    index: &PlaybackIndex,
    playback_session: &mut PlaybackSession,
    playback_tx: &watch::Sender<PlaybackState>,
    cursor: PlaybackCursor,
    started: &playback::StartedActionNarration,
) -> StepTailCompletion {
    let Some(step) = steps
        .get(cursor.step_index)
        .filter(|step| cursor.action_index + 1 >= step.actions.len() && !step.requires_human)
    else {
        return StepTailCompletion::NotApplicable;
    };
    let next = next_cursor_in_unit(steps, cursor);
    let (next_cursor, boundary) = match next {
        Some(next) if steps[next.step_index].unit_path == step.unit_path => (Some(next), None),
        _ => (
            None,
            Some(PendingBoundary::Unit {
                completed: completed_cursor(steps, cursor),
            }),
        ),
    };
    let completed = completed_cursor(steps, cursor);
    let next_step_id = next_cursor
        .and_then(|next| steps.get(next.step_index))
        .map(|step| step.step_id.as_str())
        .unwrap_or("-");
    let position =
        playback_position(steps, index, completed).expect("completed step has a position");
    log::info!(
        target: "cynapse.server",
        "step.completed correlation_id={} step_id={} unit_path={} next_step_id={} requires_human=false continuation=true",
        playback_session.correlation_id.as_deref().unwrap_or("-"),
        position.step_id,
        position.unit_path,
        next_step_id
    );
    playback_session.cursor = next_cursor;
    playback_session.state = PlaybackState::StepComplete {
        position,
        continuation: Some(NarrationClipContinuation {
            clip: started.clip.clone(),
        }),
        requires_human: false,
    };
    let _ = playback_tx.send(playback_session.state.clone());
    StepTailCompletion::Completed { boundary }
}

pub(super) async fn advance_after_successful_action(
    context: ActionAdvanceContext<'_>,
    cursor: PlaybackCursor,
) -> Result<bool, ()> {
    let advance = plan_after_successful_action(context.steps, cursor);
    let next = match &advance {
        ActionAdvance::Dispatch(_) => "action",
        ActionAdvance::Boundary(PendingBoundary::Step { .. }) => "step",
        ActionAdvance::Boundary(PendingBoundary::Unit { .. }) => "unit",
    };
    playback::log_action_completed(
        context.steps,
        cursor,
        context.playback_session.correlation_id.as_deref(),
        next,
    );
    match advance {
        ActionAdvance::Dispatch(next) => {
            context.playback_session.cursor = Some(next);
            return send_action(
                context.socket,
                context.steps,
                next,
                playback::ActionDispatchContext {
                    index: context.index,
                    active_action: context.active_action,
                    local_wait: context.local_wait,
                    skip_waits: context.playback_session.skip_waits,
                    skip_waits_through: context.playback_session.skip_waits_through,
                    skip_unit_final_wait: context.playback_session.skip_unit_final_wait,
                    action_narration: context.action_narration,
                    playback: &mut context.playback_session.state,
                    playback_tx: context.playback_tx,
                    correlation_id: context.playback_session.correlation_id.as_deref(),
                },
            )
            .await;
        }
        ActionAdvance::Boundary(boundary) => {
            let position = playback_position(
                context.steps,
                context.index,
                completed_cursor(context.steps, cursor),
            )
            .expect("active cursor has a position");
            context.playback_session.pending_boundary = Some(boundary);
            context.playback_session.state = PlaybackState::Ready {
                position: Some(position),
            };
        }
    }

    let _ = context.playback_session.advance_stored_boundary(
        context.steps,
        context.index,
        context.playback_tx,
        BoundaryProgression::Automatic,
    );
    Ok(true)
}
