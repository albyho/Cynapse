use super::*;

use super::progression::{
    ActionAdvanceContext, StepTailCompletion, advance_after_successful_action,
    current_playback_position, publish_step_completion_with_action_narration,
};

fn has_authenticated_session(state_tx: &watch::Sender<ConnectionState>) -> bool {
    matches!(*state_tx.borrow(), ConnectionState::Connected { .. })
}

pub(super) async fn authenticate(
    mut socket: WebSocketStream<TcpStream>,
    auth: &AdapterAuth,
    origin: Option<&str>,
    expected_token: &str,
    adapter_name: &'static str,
    state_tx: &watch::Sender<ConnectionState>,
) -> Option<WebSocketStream<TcpStream>> {
    // A new attempt clears a previous handshake error, but re-sending Waiting
    // while already waiting would look like an Adapter loss to the Client.
    if matches!(*state_tx.borrow(), ConnectionState::Error { .. }) {
        let _ = state_tx.send(ConnectionState::WaitingForAdapter);
    }

    let hello = tokio::time::timeout(Duration::from_secs(5), socket.next()).await;
    let Ok(Some(Ok(Message::Text(text)))) = hello else {
        return None;
    };
    let Ok(envelope) = Envelope::parse(text.as_str()) else {
        return None;
    };

    if envelope.v != PROTOCOL_VERSION {
        reject(
            &mut socket,
            &envelope.id,
            "version_mismatch",
            "unsupported protocol version",
            state_tx,
        )
        .await;
        return None;
    }

    if envelope.kind != MessageType::Hello {
        reject(
            &mut socket,
            &envelope.id,
            "invalid_message",
            "expected hello",
            state_tx,
        )
        .await;
        return None;
    }

    let Ok(payload) = serde_json::from_value::<HelloPayload>(envelope.payload.clone()) else {
        reject(
            &mut socket,
            &envelope.id,
            "invalid_message",
            "invalid hello payload",
            state_tx,
        )
        .await;
        return None;
    };

    match auth {
        AdapterAuth::Token => {
            if payload.token != expected_token {
                reject(
                    &mut socket,
                    &envelope.id,
                    "token_mismatch",
                    "invalid adapter token",
                    state_tx,
                )
                .await;
                return None;
            }
        }
        AdapterAuth::JupyterLab {
            origin: expected_origin,
        } => {
            if origin != Some(expected_origin.as_str()) {
                reject(
                    &mut socket,
                    &envelope.id,
                    "invalid_origin",
                    "unexpected adapter origin",
                    state_tx,
                )
                .await;
                return None;
            }
        }
    }

    let ack = Envelope::hello_ack(adapter_name);
    let Ok(ack_json) = ack.to_json() else {
        return None;
    };
    if socket.send(Message::Text(ack_json.into())).await.is_err() {
        return None;
    }

    let _ = state_tx.send(ConnectionState::Connected {
        adapter: adapter_name.to_string(),
    });
    log::info!(target: "cynapse.server", "adapter.connected adapter={adapter_name}");
    Some(socket)
}

pub(super) async fn handle_connection(
    mut socket: WebSocketStream<TcpStream>,
    AdapterGeneration {
        generation,
        mut receiver,
        newest,
    }: AdapterGeneration,
    state_tx: &watch::Sender<ConnectionState>,
    playback_tx: &watch::Sender<PlaybackState>,
    plan: &mut ServerPlaybackPlan,
    playback_session: &mut PlaybackSession,
    command_rx: &mut mpsc::UnboundedReceiver<PlaybackCommand>,
) {
    if !has_authenticated_session(state_tx) {
        let _ = state_tx.send(ConnectionState::Connected {
            adapter: plan.adapter_name.to_string(),
        });
    }
    let _ = playback_tx.send(playback_session.state.clone());
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + HEARTBEAT_INTERVAL,
        HEARTBEAT_INTERVAL,
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_seen = tokio::time::Instant::now();
    let mut active_action: Option<ActiveAction> = None;
    let mut local_wait: Option<LocalWait> = None;
    let mut cancellation_reason: Option<CancelReason> = None;
    let mut pending_workspace_reset: Option<(String, oneshot::Sender<Result<(), String>>)> = None;
    let mut pending_one_off: Option<(String, oneshot::Sender<Result<AckPayload, String>>)> = None;
    let mut action_narration = ActionNarrationRuntime::default();
    let mut action_deadline: Option<tokio::time::Instant> = None;
    let mut one_off_deadline: Option<tokio::time::Instant> = None;
    let mut reset_deadline: Option<tokio::time::Instant> = None;

    loop {
        let steps_plan = Arc::clone(&plan.steps);
        let steps = steps_plan.as_slice();
        let wait_deadline = local_wait.as_ref().map(|wait| wait.deadline);
        if active_action.is_some() {
            action_deadline.get_or_insert_with(|| tokio::time::Instant::now() + ACTION_ACK_TIMEOUT);
        } else {
            action_deadline = None;
        }
        if pending_one_off.is_none() {
            one_off_deadline = None;
        }
        if pending_workspace_reset.is_none() {
            reset_deadline = None;
        }
        let ack_deadline = action_deadline;
        let pending_deadline = match (one_off_deadline, reset_deadline) {
            (Some(one_off), Some(reset)) => Some(one_off.min(reset)),
            (one_off, reset) => one_off.or(reset),
        };
        tokio::select! {
            _ = wait_for_replacement(&mut receiver, generation) => {
                log::info!(
                    target: "cynapse.server",
                    "adapter.replaced generation={generation}"
                );
                let _ = socket
                    .close(Some(CloseFrame {
                        code: CloseCode::Library(INACTIVE_ADAPTER_CLOSE_CODE),
                        reason: INACTIVE_ADAPTER_REASON.into(),
                    }))
                    .await;
                break;
            }
            _ = heartbeat.tick() => {
                if last_seen.elapsed() >= HEARTBEAT_TIMEOUT {
                    // An expired deadline wins this tie so the failed request is
                    // reported as a timeout instead of silently dropped.
                    let now = tokio::time::Instant::now();
                    if ack_deadline.is_some_and(|deadline| deadline <= now)
                        || one_off_deadline.is_some_and(|deadline| deadline <= now)
                        || reset_deadline.is_some_and(|deadline| deadline <= now)
                    {
                        continue;
                    }
                    log::warn!(target: "cynapse.server", "adapter.heartbeat_timeout");
                    break;
                }
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            message = socket.next() => {
                if matches!(&message, Some(Ok(_))) {
                    last_seen = tokio::time::Instant::now();
                }
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(envelope) = Envelope::parse(text.as_str())
                            && envelope.kind == MessageType::Ack
                            && let Ok(payload) =
                                serde_json::from_value::<AckPayload>(envelope.payload)
                        {
                                    let ack_code = payload.code.as_deref().unwrap_or("-");
                                    let ack_message = payload.message.as_deref().unwrap_or("-");
                                    log::info!(
                                        target: "cynapse.server",
                                        "protocol.acked correlation_id={} envelope_id={} status={:?} code={} message={}",
                                        playback_session
                                            .correlation_id
                                            .as_deref()
                                            .unwrap_or("-"),
                                        envelope.id,
                                        payload.status,
                                        ack_code,
                                        ack_message
                                    );
                                    if let Some((expected_id, completion)) =
                                        pending_one_off.take()
                                    {
                                        if expected_id == envelope.id {
                                            log::info!(
                                                target: "cynapse.server",
                                                "one_off.completed correlation_id={} request_id={} status={:?} code={}",
                                                playback_session
                                                    .correlation_id
                                                    .as_deref()
                                                    .unwrap_or("-"),
                                                envelope.id,
                                                payload.status,
                                                payload.code.as_deref().unwrap_or("-")
                                            );
                                            let _ = completion.send(Ok(payload));
                                            continue;
                                        }
                                        pending_one_off = Some((expected_id, completion));
                                    }

                                    if let Some((expected_id, completion)) =
                                        pending_workspace_reset.take()
                                    {
                                        if expected_id == envelope.id {
                                            let status = payload.status;
                                            let result = match status {
                                                AckStatus::Done => Ok(()),
                                                AckStatus::Error => Err(payload
                                                    .message
                                                    .unwrap_or_else(|| {
                                                        payload.code.unwrap_or_else(|| {
                                                            "workspace reset failed".to_string()
                                                        })
                                                    })),
                                                AckStatus::Cancelled => {
                                                    Err("workspace reset was cancelled".to_string())
                                                }
                                            };
                                            if result.is_ok() {
                                                log::info!(
                                                    target: "cynapse.server",
                                                    "workspace_reset.completed correlation_id={} request_id={} status={:?} ok=true",
                                                    playback_session
                                                        .correlation_id
                                                        .as_deref()
                                                        .unwrap_or("-"),
                                                    envelope.id,
                                                    status
                                                );
                                            } else {
                                                log::error!(
                                                    target: "cynapse.server",
                                                    "workspace_reset.completed correlation_id={} request_id={} status={:?} ok=false",
                                                    playback_session
                                                        .correlation_id
                                                        .as_deref()
                                                        .unwrap_or("-"),
                                                    envelope.id,
                                                    status
                                                );
                                            }
                                            let _ = completion.send(result);
                                            continue;
                                        }
                                        pending_workspace_reset =
                                            Some((expected_id, completion));
                                    }

                                    let Some(active) = active_action.take() else {
                                        continue;
                                    };
                                    if active.id != envelope.id {
                                        active_action = Some(active);
                                        continue;
                                    }
                                    action_deadline = None;

                                    playback_session.pending_boundary = None;
                                    let next_state = match payload.status {
                                        AckStatus::Done => {
                                            cancellation_reason = None;
                                            match action_narration.action_done(active.cursor) {
                                                Some(
                                                    playback::ActionNarrationDone::WaitForDuringStart,
                                                ) => continue,
                                                Some(
                                                    playback::ActionNarrationDone::WaitForDuring(
                                                        started,
                                                    ),
                                                ) => {
                                                    if let StepTailCompletion::Completed {
                                                        boundary,
                                                    } =
                                                        publish_step_completion_with_action_narration(
                                                            steps,
                                                            &plan.index,
                                                            playback_session,
                                                            playback_tx,
                                                            active.cursor,
                                                            &started,
                                                        )
                                                    {
                                                        playback_session.pending_boundary = boundary;
                                                        continue;
                                                    }
                                                    if playback::publish_action_narration(
                                                        steps,
                                                        &plan.index,
                                                        &mut playback_session.state,
                                                        playback_tx,
                                                        &started,
                                                        playback_session
                                                            .correlation_id
                                                            .as_deref(),
                                                    )
                                                    .is_err()
                                                    {
                                                        break;
                                                    }
                                                    continue;
                                                }
                                                Some(
                                                    playback::ActionNarrationDone::StartAfter(
                                                        started,
                                                    ),
                                                ) => {
                                                    if playback::publish_action_narration(
                                                        steps,
                                                        &plan.index,
                                                        &mut playback_session.state,
                                                        playback_tx,
                                                        &started,
                                                        playback_session
                                                            .correlation_id
                                                            .as_deref(),
                                                    )
                                                    .is_err()
                                                    {
                                                        break;
                                                    }
                                                    continue;
                                                }
                                                Some(
                                                    playback::ActionNarrationDone::Complete,
                                                )
                                                | None => {}
                                            }
                                            if !advance_after_successful_action(
                                                ActionAdvanceContext {
                                                    socket: &mut socket,
                                                    steps,
                                                    index: &plan.index,
                                                    playback_session,
                                                    action_narration: &mut action_narration,
                                                    active_action: &mut active_action,
                                                    local_wait: &mut local_wait,
                                                    playback_tx,
                                                },
                                                active.cursor,
                                            )
                                            .await
                                            .unwrap_or(false)
                                            {
                                                break;
                                            }
                                            continue;
                                        }
                                        AckStatus::Cancelled => {
                                            action_narration.clear();
                                            let timed_out = ack_timed_out(&payload);
                                            let requested_reason: PlaybackInterruptionReason =
                                                cancellation_reason
                                                    .take()
                                                    .unwrap_or(
                                                        CancelReason::UserStop,
                                                    )
                                                    .into();
                                            let reason = if payload.code.as_deref()
                                                == Some("kernel_restarted")
                                            {
                                                PlaybackInterruptionReason::JupyterLabKernelRestarted
                                            } else if timed_out {
                                                PlaybackInterruptionReason::Timeout
                                            } else {
                                                requested_reason
                                            };
                                            let step = steps.get(active.cursor.step_index);
                                            if timed_out {
                                                log::error!(
                                                    target: "cynapse.server",
                                                    "action.timeout correlation_id={} action_id={} step_id={} unit_path={} action_index={} code={} message={}",
                                                    playback_session
                                                        .correlation_id
                                                        .as_deref()
                                                        .unwrap_or("-"),
                                                    active.id,
                                                    step.map(|step| step.step_id.as_str()).unwrap_or("-"),
                                                    step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
                                                    active.cursor.action_index,
                                                    payload.code.as_deref().unwrap_or("-"),
                                                    payload.message.as_deref().unwrap_or("-")
                                                );
                                            }
                                            log::info!(
                                                target: "cynapse.server",
                                                "action.cancelled correlation_id={} action_id={} step_id={} unit_path={} action_index={} reason={:?} timed_out={}",
                                                playback_session
                                                    .correlation_id
                                                    .as_deref()
                                                    .unwrap_or("-"),
                                                active.id,
                                                step.map(|step| step.step_id.as_str()).unwrap_or("-"),
                                                step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
                                                active.cursor.action_index,
                                                reason,
                                                timed_out
                                            );
                                            let interruption = interruption_from_payload(
                                                &payload,
                                                reason,
                                            );
                                            let error = timed_out.then(|| {
                                                (
                                                    payload.code.unwrap_or_default(),
                                                    payload.message.unwrap_or_default(),
                                                )
                                            });
                                            playback_session.interrupted_action(
                                                active.cursor,
                                                steps,
                                                &plan.index,
                                                interruption,
                                                error,
                                            )
                                        }
                                        AckStatus::Error => {
                                            cancellation_reason = None;
                                            action_narration.clear();
                                            let interrupted_position =
                                                playback_position(steps, &plan.index, active.cursor);
                                            let code = payload
                                                .code
                                                .clone()
                                                .unwrap_or_else(|| "action_failed".to_string());
                                            let message =
                                                payload.message.clone().unwrap_or_default();
                                            let timed_out = ack_timed_out(&payload);
                                            let step = steps.get(active.cursor.step_index);
                                            log::error!(
                                                target: "cynapse.server",
                                                "action.failed correlation_id={} action_id={} step_id={} unit_path={} action_index={} code={} message={} timed_out={}",
                                                playback_session
                                                    .correlation_id
                                                    .as_deref()
                                                    .unwrap_or("-"),
                                                active.id,
                                                step.map(|step| step.step_id.as_str()).unwrap_or("-"),
                                                step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
                                                active.cursor.action_index,
                                                code,
                                                message,
                                                timed_out
                                            );
                                            let reason = if timed_out {
                                                PlaybackInterruptionReason::Timeout
                                            } else {
                                                PlaybackInterruptionReason::CommandFailure
                                            };
                                            let interruption =
                                                interruption_from_payload(&payload, reason);
                                            playback_session.failed_action(
                                                Some(active.cursor),
                                                interrupted_position,
                                                code,
                                                message,
                                                Some(interruption),
                                            )
                                        }
                                    };
                                    playback_session.state = next_state;
                                    if matches!(
                                        playback_session.advance_stored_boundary(
                                            steps,
                                            &plan.index,
                                            playback_tx,
                                            BoundaryProgression::Automatic,
                                        ),
                                        BoundaryAdvance::NoBoundary
                                    ) {
                                        let _ = playback_tx.send(
                                            playback_session.state.clone(),
                                        );
                                    }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(wait_deadline.unwrap_or_else(tokio::time::Instant::now)), if wait_deadline.is_some() => {
                let Some(wait) = local_wait.take() else { continue; };
                let Some(active) = active_action.take() else { continue; };
                if active.cursor != wait.cursor {
                    active_action = Some(active);
                    local_wait = Some(wait);
                    continue;
                }
                action_deadline = None;
                // The effective Wait deadline is `max(duration_ms, audio clip)`.
                // It is also the liveness fallback: a Wait audio clip that starts
                // but never reports its end must not hold the Wait forever.
                // Clear the stale clip so a late ended event cannot advance twice.
                action_narration.complete_active_beat(wait.cursor);
                if !advance_after_successful_action(
                    ActionAdvanceContext {
                        socket: &mut socket,
                        steps,
                        index: &plan.index,
                        playback_session,
                        action_narration: &mut action_narration,
                        active_action: &mut active_action,
                        local_wait: &mut local_wait,
                        playback_tx,
                    },
                    wait.cursor,
                )
                .await
                .unwrap_or(false)
                {
                    break;
                }
            }
            _ = tokio::time::sleep_until(ack_deadline.unwrap_or_else(tokio::time::Instant::now)), if ack_deadline.is_some() => {
                let Some(active) = active_action.take() else { continue; };
                action_deadline = None;
                action_narration.clear();
                local_wait = None;
                let step = steps.get(active.cursor.step_index);
                log::error!(
                    target: "cynapse.server",
                    "action.failed correlation_id={} action_id={} step_id={} unit_path={} action_index={} code=action_timeout message={} timed_out=true",
                    playback_session
                        .correlation_id
                        .as_deref()
                        .unwrap_or("-"),
                    active.id,
                    step.map(|step| step.step_id.as_str()).unwrap_or("-"),
                    step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
                    active.cursor.action_index,
                    "The adapter did not acknowledge the action in time"
                );
                // The protocol has no timeout-specific cancel reason; stop the
                // adapter work before reporting the timeout locally.
                let cancel =
                    Envelope::cancel_with_reason(active.id.clone(), CancelReason::UserStop);
                if let Ok(json) = cancel.to_json() {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
                playback_session.state = playback_session.interrupted_action(
                    active.cursor,
                    steps,
                    &plan.index,
                    PlaybackInterruption {
                        reason: PlaybackInterruptionReason::Timeout,
                        exit_code: None,
                        output: None,
                        output_truncated: None,
                    },
                    Some((
                        "action_timeout".to_string(),
                        "The adapter did not acknowledge the action in time".to_string(),
                    )),
                );
                let _ = playback_tx.send(playback_session.state.clone());
            }
            _ = tokio::time::sleep_until(pending_deadline.unwrap_or_else(tokio::time::Instant::now)), if pending_deadline.is_some() => {
                let now = tokio::time::Instant::now();
                if one_off_deadline.is_some_and(|deadline| deadline <= now)
                    && let Some((_, completion)) = pending_one_off.take()
                {
                    let _ = completion.send(Err(
                        "adapter did not acknowledge the action in time".to_string()
                    ));
                }
                if reset_deadline.is_some_and(|deadline| deadline <= now)
                    && let Some((_, completion)) = pending_workspace_reset.take()
                {
                    let _ = completion.send(Err(
                        "adapter did not acknowledge the workspace reset in time".to_string()
                    ));
                }
            }
            command = command_rx.recv() => {
                let Some(command) = command else { break; };
                match command {
                    PlaybackCommand::SetSkipUnitFinalWait(skip) => {
                        playback_session.skip_unit_final_wait = skip;
                    }
                    PlaybackCommand::SetSkipWaits(skip) => {
                        playback_session.force_skip_waits = skip;
                        playback_session.skip_waits = skip;
                    }
                    PlaybackCommand::SetCorrelation { correlation_id } => {
                        log::info!(
                            target: "cynapse.server",
                            "correlation.set correlation_id={correlation_id}"
                        );
                        playback_session.correlation_id = Some(correlation_id);
                    }
                    PlaybackCommand::Play => {
                        if active_action.is_some() {
                            log::debug!(
                                target: "cynapse.server",
                                "playback.play ignored reason=action_in_flight"
                            );
                            continue;
                        }
                        // Dropping a closed slot only stops waiting; the Adapter
                        // still runs the request, serialized on its queue.
                        if pending_one_off
                            .as_ref()
                            .is_some_and(|(_, completion)| completion.is_closed())
                        {
                            pending_one_off = None;
                        }
                        if pending_workspace_reset
                            .as_ref()
                            .is_some_and(|(_, completion)| completion.is_closed())
                        {
                            pending_workspace_reset = None;
                        }
                        if pending_one_off.is_some() {
                            log::debug!(
                                target: "cynapse.server",
                                "playback.play ignored reason=one_off_in_flight"
                            );
                            continue;
                        }
                        if pending_workspace_reset.is_some() {
                            log::debug!(
                                target: "cynapse.server",
                                "playback.play ignored reason=workspace_reset_in_flight"
                            );
                            continue;
                        }
                        log::info!(
                            target: "cynapse.server",
                            "playback.play correlation_id={} state={}",
                            playback_session
                                .correlation_id
                                .as_deref()
                                .unwrap_or("-"),
                            playback_state_kind(&playback_session.state)
                        );
                        if matches!(
                            &playback_session.state,
                            PlaybackState::StepComplete { .. }
                        ) {
                            action_narration.clear();
                            if !matches!(
                                playback_session.advance_stored_boundary(
                                    steps,
                                    &plan.index,
                                    playback_tx,
                                    BoundaryProgression::Explicit,
                                ),
                                BoundaryAdvance::NoBoundary
                            ) {
                                continue;
                            }
                        }
                        let target = match &playback_session.state {
                            PlaybackState::Ready { position } => {
                                position.as_ref().and(playback_session.cursor)
                            }
                            PlaybackState::Paused { .. }
                            | PlaybackState::StepComplete { .. } => {
                                playback_session.cursor
                            }
                            PlaybackState::UnitComplete { .. } => None,
                            PlaybackState::Error {
                                position: Some(_),
                                ..
                            } => playback_session.cursor,
                            _ => None,
                        };
                        if let Some(target) = target
                            && !send_action(
                                &mut socket,
                                steps,
                                target,
                                ActionDispatchContext {
                                    index: &plan.index,
                                    active_action: &mut active_action,
                                    local_wait: &mut local_wait,
                                    skip_waits: playback_session.skip_waits,
                                    skip_waits_through: playback_session.skip_waits_through,
                                    skip_unit_final_wait: playback_session.skip_unit_final_wait,
                                    action_narration: &mut action_narration,
                                    playback: &mut playback_session.state,
                                    playback_tx,
                                    correlation_id: playback_session.correlation_id.as_deref(),
                                },
                            )
                            .await
                            .unwrap_or(false)
                        {
                            break;
                        }
                    }
                    PlaybackCommand::Cancel { reason } => {
                        log::info!(
                            target: "cynapse.server",
                            "playback.cancel_requested correlation_id={} reason={:?}",
                            playback_session
                                .correlation_id
                                .as_deref()
                                .unwrap_or("-"),
                            reason
                        );
                        if let Some(wait) = local_wait.take() {
                            active_action.take();
                            action_deadline = None;
                            action_narration.clear();
                            let interrupted_position =
                                playback_position(steps, &plan.index, wait.cursor);
                            playback_session.restart_at(wait.cursor);
                            let position = interrupted_position
                                .expect("local Wait has a position");
                            log::info!(
                                target: "cynapse.server",
                                "playback.paused correlation_id={} reason={:?} step_id={} unit_path={} action_index={}",
                                playback_session
                                    .correlation_id
                                    .as_deref()
                                    .unwrap_or("-"),
                                reason,
                                position.step_id,
                                position.unit_path,
                                wait.cursor.action_index
                            );
                            playback_session.state = PlaybackState::Paused {
                                position,
                                interruption: PlaybackInterruption {
                                    reason: reason.into(),
                                    exit_code: None,
                                    output: None,
                                    output_truncated: None,
                                },
                            };
                            let _ = playback_tx.send(playback_session.state.clone());
                        } else if let Some(active) = active_action.as_ref() {
                            cancellation_reason = Some(reason);
                            let cancel =
                                Envelope::cancel_with_reason(active.id.clone(), reason);
                            let Ok(json) = cancel.to_json() else { continue; };
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        } else if let Some((id, completion)) = pending_one_off.take() {
                            cancellation_reason = Some(reason);
                            let cancel = Envelope::cancel_with_reason(id, reason);
                            let Ok(json) = cancel.to_json() else { continue; };
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                let _ = completion
                                    .send(Err("adapter disconnected while cancelling".to_string()));
                                break;
                            }
                            let _ = completion.send(Err("action was cancelled".to_string()));
                        } else if let PlaybackState::Narrating { position, .. }
                        | PlaybackState::StepComplete {
                            position,
                            continuation: Some(_),
                            ..
                        } = &playback_session.state
                        {
                            action_narration.clear();
                            playback_session.pending_boundary = None;
                            log::info!(
                                target: "cynapse.server",
                                "playback.paused correlation_id={} reason={:?} step_id={} unit_path={} action_index={}",
                                playback_session
                                    .correlation_id
                                    .as_deref()
                                    .unwrap_or("-"),
                                reason,
                                position.step_id,
                                position.unit_path,
                                position.action_index
                            );
                            playback_session.state = PlaybackState::Paused {
                                position: position.clone(),
                                interruption: PlaybackInterruption {
                                    reason: reason.into(),
                                    exit_code: None,
                                    output: None,
                                    output_truncated: None,
                                },
                            };
                            let _ = playback_tx.send(playback_session.state.clone());
                        }
                    }
                    PlaybackCommand::CancelPendingOneOff { reason } => {
                        if let Some((id, completion)) = pending_one_off.take() {
                            let cancel = Envelope::cancel_with_reason(id, reason);
                            if let Ok(json) = cancel.to_json()
                                && socket.send(Message::Text(json.into())).await.is_err()
                            {
                                let _ = completion
                                    .send(Err("adapter disconnected while cancelling".to_string()));
                                break;
                            }
                            let _ = completion.send(Err("action was cancelled".to_string()));
                        }
                    }
                    PlaybackCommand::WorkspaceReset {
                        paths,
                        exclude_paths,
                        session_keys,
                        close_tabs,
                        completion,
                    } => {
                        if pending_workspace_reset
                            .as_ref()
                            .is_some_and(|(_, completion)| completion.is_closed())
                        {
                            pending_workspace_reset = None;
                        }
                        if pending_workspace_reset.is_some() {
                            let _ = completion
                                .send(Err("another workspace reset is in progress".to_string()));
                            continue;
                        }
                        let envelope = if close_tabs {
                            Envelope::workspace_reset(paths, exclude_paths)
                        } else if session_keys.is_empty() {
                            Envelope::terminal_cleanup(paths, exclude_paths)
                        } else {
                            Envelope::terminal_cleanup_sessions(session_keys)
                        };
                        let Ok(json) = envelope.to_json() else { continue; };
                        let id = envelope.id.clone();
                        log::info!(
                            target: "cynapse.server",
                            "workspace_reset.started correlation_id={} request_id={}",
                            playback_session
                                .correlation_id
                                .as_deref()
                                .unwrap_or("-"),
                            id
                        );
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            let _ = completion
                                .send(Err("adapter disconnected during workspace reset".to_string()));
                            break;
                        }
                        pending_workspace_reset = Some((id, completion));
                        reset_deadline =
                            Some(tokio::time::Instant::now() + ACTION_ACK_TIMEOUT);
                    }
                    PlaybackCommand::ExecuteAction {
                        action,
                        unit_path,
                        completion,
                    } => {
                        if pending_one_off
                            .as_ref()
                            .is_some_and(|(_, completion)| completion.is_closed())
                        {
                            pending_one_off = None;
                        }
                        if active_action.is_some() || pending_one_off.is_some() {
                            let _ = completion
                                .send(Err("another action is already in progress".to_string()));
                            continue;
                        }
                        if unit_path.trim().is_empty() {
                            let _ = completion
                                .send(Err("one-off action is missing its unit path".to_string()));
                            continue;
                        }
                        let payload = match serde_json::to_value(action) {
                            Ok(payload) => payload,
                            Err(error) => {
                                let _ = completion.send(Err(error.to_string()));
                                continue;
                            }
                        };
                        let mut envelope = Envelope::new(MessageType::Action, payload);
                        envelope.unit_path = Some(unit_path);
                        let id = envelope.id.clone();
                        log::info!(
                            target: "cynapse.server",
                            "one_off.action.dispatch correlation_id={} request_id={} unit_path={} type={} path={}",
                            playback_session
                                .correlation_id
                                .as_deref()
                                .unwrap_or("-"),
                            id,
                            envelope.unit_path.as_deref().unwrap_or("-"),
                            envelope
                                .payload
                                .get("type")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("-"),
                            envelope
                                .payload
                                .get("path")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("-")
                        );
                        let Ok(json) = envelope.to_json() else {
                            let _ = completion.send(Err("could not serialize action".to_string()));
                            continue;
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            let _ = completion
                                .send(Err("adapter disconnected before the action".to_string()));
                            break;
                        }
                        pending_one_off = Some((id, completion));
                        one_off_deadline =
                            Some(tokio::time::Instant::now() + ACTION_ACK_TIMEOUT);
                    }
                    PlaybackCommand::ExecuteCheck { check, completion } => {
                        if pending_one_off
                            .as_ref()
                            .is_some_and(|(_, completion)| completion.is_closed())
                        {
                            pending_one_off = None;
                        }
                        if active_action.is_some() || pending_one_off.is_some() {
                            let _ = completion
                                .send(Err("another action is already in progress".to_string()));
                            continue;
                        }
                        let envelope = Envelope::check(check);
                        let id = envelope.id.clone();
                        log::info!(
                            target: "cynapse.server",
                            "one_off.check.dispatch correlation_id={} request_id={} unit_path={} type={}",
                            playback_session
                                .correlation_id
                                .as_deref()
                                .unwrap_or("-"),
                            id,
                            envelope
                                .payload
                                .get("unit_path")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("-"),
                            envelope
                                .payload
                                .get("type")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("-")
                        );
                        let Ok(json) = envelope.to_json() else {
                            let _ = completion.send(Err("could not serialize check".to_string()));
                            continue;
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            let _ = completion
                                .send(Err("adapter disconnected before the check".to_string()));
                            break;
                        }
                        pending_one_off = Some((id, completion));
                        one_off_deadline =
                            Some(tokio::time::Instant::now() + ACTION_ACK_TIMEOUT);
                    }
                    PlaybackCommand::ReplacePlan {
                        steps: replacement_steps,
                        resume,
                        completion,
                    } => {
                        if pending_one_off
                            .as_ref()
                            .is_some_and(|(_, completion)| completion.is_closed())
                        {
                            pending_one_off = None;
                        }
                        if pending_workspace_reset
                            .as_ref()
                            .is_some_and(|(_, completion)| completion.is_closed())
                        {
                            pending_workspace_reset = None;
                        }
                        if active_action.is_some()
                            || pending_one_off.is_some()
                            || pending_workspace_reset.is_some()
                        {
                            let _ = completion
                                .send(Err("playback transition is still in progress".to_string()));
                            continue;
                        }
                        let replacement_plan =
                            ServerPlaybackPlan::new(replacement_steps, plan.adapter_name);
                        playback_session.cursor =
                            initial_cursor(&replacement_plan, resume.as_ref());
                        playback_session.state =
                            initial_playback_state(&replacement_plan, resume.as_ref());
                        playback_session.skip_waits = playback_session.force_skip_waits;
                        playback_session.skip_waits_through =
                            resume_skip_waits_through(&replacement_plan, resume.as_ref());
                        *plan = replacement_plan;
                        action_narration.clear();
                        local_wait = None;
                        playback_session.pending_boundary = None;
                        cancellation_reason = None;
                        log::info!(
                            target: "cynapse.server",
                            "playback.plan.replaced correlation_id={} steps={} actions={} resume_step={} resume_unit={}",
                            playback_session
                                .correlation_id
                                .as_deref()
                                .unwrap_or("-"),
                            plan.steps.len(),
                            plan.index.total_actions(),
                            resume.as_ref().map(|resume| resume.step_id.as_str()).unwrap_or("-"),
                            resume.as_ref().map(|resume| resume.unit_path.as_str()).unwrap_or("-")
                        );
                        let state = playback_session.state.clone();
                        let _ = playback_tx.send(state);
                        let _ = completion.send(Ok(()));
                    }
                    PlaybackCommand::NarrationStarted { clip } => {
                        if action_narration.matches(&clip) {
                            let phase = action_narration.mark_started(&clip);
                            let Some(cursor) = action_narration.cursor() else {
                                continue;
                            };
                            if phase.is_some() {
                                let step = steps.get(cursor.step_index);
                                log::info!(
                                    target: "cynapse.server",
                                    "action.narration.confirmed correlation_id={} step_id={} unit_path={} action_index={} phase={:?} step_run_id={} clip_index={}",
                                    playback_session
                                        .correlation_id
                                        .as_deref()
                                        .unwrap_or("-"),
                                    step.map(|step| step.step_id.as_str()).unwrap_or("-"),
                                    step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
                                    cursor.action_index,
                                    clip.phase,
                                    clip.step_run_id,
                                    clip.clip_index
                                );
                            }
                            match phase {
                                Some(ActionNarrationPhase::During) => {
                                    if clip.clip_index == 0
                                        && !dispatch_action(
                                        &mut socket,
                                        steps,
                                        &plan.index,
                                        cursor,
                                        &mut active_action,
                                        &mut local_wait,
                                        playback_session.skip_waits,
                                        playback_session.skip_waits_through,
                                        playback_session.skip_unit_final_wait,
                                        &mut playback_session.state,
                                        playback_tx,
                                        playback_session.correlation_id.as_deref(),
                                    )
                                        .await
                                        .unwrap_or(false)
                                    {
                                        break;
                                    }
                                    if clip.clip_index > 0 && action_narration.action_is_done() {
                                        let started = playback::StartedActionNarration {
                                            cursor,
                                            clip: clip.clone(),
                                        };
                                        if let StepTailCompletion::Completed { boundary } =
                                            publish_step_completion_with_action_narration(
                                                steps,
                                                &plan.index,
                                                playback_session,
                                                playback_tx,
                                                cursor,
                                                &started,
                                            )
                                        {
                                            playback_session.pending_boundary = boundary;
                                        }
                                    }
                                }
                                Some(ActionNarrationPhase::After) => {
                                    let started = playback::StartedActionNarration {
                                        cursor,
                                        clip: clip.clone(),
                                    };
                                    if let StepTailCompletion::Completed { boundary } =
                                        publish_step_completion_with_action_narration(
                                            steps,
                                            &plan.index,
                                            playback_session,
                                            playback_tx,
                                            cursor,
                                            &started,
                                        )
                                    {
                                        playback_session.pending_boundary = boundary;
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }
                    }
                    PlaybackCommand::NarrationEnded { clip } => {
                        if action_narration.matches(&clip) {
                            let Some(end) = action_narration.end_active(&clip) else {
                                continue;
                            };
                            let Some(cursor) = action_narration.cursor() else {
                                continue;
                            };
                            let completed_tail_boundary = match &playback_session.state {
                                PlaybackState::StepComplete {
                                    position,
                                    continuation: Some(continuation),
                                    requires_human,
                                } if continuation.clip == clip =>
                                {
                                    Some((position.clone(), *requires_human))
                                }
                                _ => None,
                            };
                            if completed_tail_boundary.is_none() {
                                playback_session.cursor = Some(cursor);
                            }
                            let next = match &end {
                                playback::ActionNarrationEnd::BeforeNext(_)
                                | playback::ActionNarrationEnd::DuringNext(_)
                                | playback::ActionNarrationEnd::AfterNext(_)
                                | playback::ActionNarrationEnd::BeforeEnded(Some(_)) => "narration",
                                playback::ActionNarrationEnd::BeforeEnded(None) => "action",
                                playback::ActionNarrationEnd::DuringBeforeAction => "action_ack",
                                playback::ActionNarrationEnd::DuringAfterAction
                                | playback::ActionNarrationEnd::AfterComplete
                                    if completed_tail_boundary.is_some() =>
                                {
                                    "step_complete"
                                }
                                playback::ActionNarrationEnd::DuringAfterAction
                                | playback::ActionNarrationEnd::AfterComplete
                                    if local_wait
                                        .as_ref()
                                        .is_some_and(|wait| wait.cursor == cursor) =>
                                {
                                    "wait"
                                }
                                playback::ActionNarrationEnd::DuringAfterAction
                                | playback::ActionNarrationEnd::AfterComplete => "advance",
                            };
                            let step = steps.get(cursor.step_index);
                            log::info!(
                                target: "cynapse.server",
                                "action.narration.ended correlation_id={} step_id={} unit_path={} action_index={} phase={:?} step_run_id={} clip_index={} next={}",
                                playback_session
                                    .correlation_id
                                    .as_deref()
                                    .unwrap_or("-"),
                                step.map(|step| step.step_id.as_str()).unwrap_or("-"),
                                step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
                                cursor.action_index,
                                clip.phase,
                                clip.step_run_id,
                                clip.clip_index,
                                next
                            );
                            match end {
                                playback::ActionNarrationEnd::BeforeNext(started)
                                | playback::ActionNarrationEnd::DuringNext(started)
                                | playback::ActionNarrationEnd::AfterNext(started) => {
                                    if playback::publish_action_narration(
                                        steps,
                                        &plan.index,
                                        &mut playback_session.state,
                                        playback_tx,
                                        &started,
                                        playback_session.correlation_id.as_deref(),
                                    )
                                    .is_err()
                                    {
                                        break;
                                    }
                                }
                                playback::ActionNarrationEnd::BeforeEnded(Some(started)) => {
                                        if playback::publish_action_narration(
                                            steps,
                                            &plan.index,
                                            &mut playback_session.state,
                                            playback_tx,
                                            &started,
                                            playback_session.correlation_id.as_deref(),
                                        )
                                        .is_err()
                                        {
                                            break;
                                        }
                                }
                                playback::ActionNarrationEnd::BeforeEnded(None) => {
                                    if !dispatch_action(
                                        &mut socket,
                                        steps,
                                        &plan.index,
                                        cursor,
                                        &mut active_action,
                                        &mut local_wait,
                                        playback_session.skip_waits,
                                        playback_session.skip_waits_through,
                                        playback_session.skip_unit_final_wait,
                                        &mut playback_session.state,
                                        playback_tx,
                                        playback_session.correlation_id.as_deref(),
                                    )
                                    .await
                                    .unwrap_or(false)
                                    {
                                        break;
                                    }
                                }
                                playback::ActionNarrationEnd::DuringBeforeAction => {}
                                playback::ActionNarrationEnd::DuringAfterAction
                                | playback::ActionNarrationEnd::AfterComplete => {
                                    action_narration.complete_active_beat(cursor);
                                    // A Wait's own timer owns its advance; a short
                                    // audio clip ending first must not shorten it.
                                    if local_wait
                                        .as_ref()
                                        .is_some_and(|wait| wait.cursor == cursor)
                                    {
                                        continue;
                                    }
                                    if let Some((position, requires_human)) =
                                        completed_tail_boundary
                                    {
                                        playback::log_action_completed(
                                            steps,
                                            cursor,
                                            playback_session.correlation_id.as_deref(),
                                            "step_complete",
                                        );
                                        playback_session.state = PlaybackState::StepComplete {
                                            position,
                                            continuation: None,
                                            requires_human,
                                        };
                                        if matches!(
                                            playback_session.advance_stored_boundary(
                                                steps,
                                                &plan.index,
                                                playback_tx,
                                                BoundaryProgression::Automatic,
                                            ),
                                            BoundaryAdvance::NoBoundary
                                        ) {
                                            let _ = playback_tx
                                                .send(playback_session.state.clone());
                                        }
                                        continue;
                                    }
                                    if !advance_after_successful_action(
                                        ActionAdvanceContext {
                                            socket: &mut socket,
                                            steps,
                                            index: &plan.index,
                                            playback_session,
                                            action_narration: &mut action_narration,
                                            active_action: &mut active_action,
                                            local_wait: &mut local_wait,
                                            playback_tx,
                                        },
                                        cursor,
                                    )
                                    .await
                                    .unwrap_or(false)
                                    {
                                        break;
                                    }
                                }
                            }
                            continue;
                        }
                    }
                    PlaybackCommand::NarrationFailed { clip, message } => {
                        if action_narration.matches(&clip) {
                            let interrupted_cursor = action_narration.cursor();
                            let interrupted_position = interrupted_cursor
                                .and_then(|cursor| playback_position(steps, &plan.index, cursor));
                            let step = interrupted_cursor.and_then(|cursor| {
                                steps.get(cursor.step_index).map(|step| (cursor, step))
                            });
                            log::error!(
                                target: "cynapse.server",
                                "action.narration.failed correlation_id={} step_id={} unit_path={} action_index={} phase={:?} step_run_id={} clip_index={} message={}",
                                playback_session
                                    .correlation_id
                                    .as_deref()
                                    .unwrap_or("-"),
                                step.map(|(_, step)| step.step_id.as_str()).unwrap_or("-"),
                                step.map(|(_, step)| step.unit_path.as_str()).unwrap_or("-"),
                                step.map(|(cursor, _)| cursor.action_index).unwrap_or(0),
                                clip.phase,
                                clip.step_run_id,
                                clip.clip_index,
                                message
                            );
                            if let Some(active) = active_action.take() {
                                action_deadline = None;
                                let cancel = Envelope::cancel_with_reason(
                                    active.id,
                                    CancelReason::UserStop,
                                );
                                if let Ok(json) = cancel.to_json()
                                    && socket.send(Message::Text(json.into())).await.is_err()
                                {
                                    break;
                                }
                            }
                            action_narration.clear();
                            playback_session.state = playback_session.failed_action(
                                interrupted_cursor,
                                interrupted_position,
                                "narration_failed".to_string(),
                                message,
                                Some(PlaybackInterruption {
                                    reason: PlaybackInterruptionReason::MediaFailure,
                                    exit_code: None,
                                    output: None,
                                    output_truncated: None,
                                }),
                            );
                            let _ = playback_tx.send(playback_session.state.clone());
                            continue;
                        }
                    }
                    PlaybackCommand::PersistenceFailed { message } => {
                        let interrupted_cursor = active_action
                            .as_ref()
                            .map(|active| active.cursor)
                            .or(playback_session.cursor);
                        let interrupted_position = interrupted_cursor
                            .and_then(|cursor| playback_position(steps, &plan.index, cursor))
                            .or_else(|| current_playback_position(&playback_session.state));
                        let step = interrupted_cursor
                            .and_then(|cursor| steps.get(cursor.step_index).map(|step| (cursor, step)));
                        log::error!(
                            target: "cynapse.server",
                            "playback.error correlation_id={} code=persistence_failed step_id={} unit_path={} action_index={} message={}",
                            playback_session
                                .correlation_id
                                .as_deref()
                                .unwrap_or("-"),
                            step.map(|(_, step)| step.step_id.as_str()).unwrap_or("-"),
                            step.map(|(_, step)| step.unit_path.as_str()).unwrap_or("-"),
                            step.map(|(cursor, _)| cursor.action_index).unwrap_or(0),
                            message
                        );
                        if let Some(active) = active_action.take() {
                            action_deadline = None;
                            let cancel = Envelope::cancel_with_reason(
                                active.id,
                                CancelReason::UserStop,
                            );
                            if let Ok(json) = cancel.to_json()
                                && socket.send(Message::Text(json.into())).await.is_err()
                            {
                                break;
                            }
                        }
                        action_narration.clear();
                        cancellation_reason = None;
                        playback_session.state = playback_session.failed_action(
                            interrupted_cursor,
                            interrupted_position,
                            "persistence_failed".to_string(),
                            message,
                            None,
                        );
                        let _ = playback_tx.send(playback_session.state.clone());
                    }
                }
            }
        }
    }

    if let Some((_, completion)) = pending_workspace_reset {
        let _ = completion.send(Err(
            "adapter disconnected during workspace reset".to_string()
        ));
    }
    if let Some((_, completion)) = pending_one_off {
        let _ = completion.send(Err("adapter disconnected before acknowledging".to_string()));
    }
    if let Some(active) = active_action {
        let interrupted_position =
            playback_position(plan.steps.as_slice(), &plan.index, active.cursor);
        let step = plan.steps.get(active.cursor.step_index);
        log::info!(
            target: "cynapse.server",
            "playback.paused correlation_id={} reason=adapter_disconnected action_id={} step_id={} unit_path={} action_index={}",
            playback_session
                .correlation_id
                .as_deref()
                .unwrap_or("-"),
            active.id,
            step.map(|step| step.step_id.as_str()).unwrap_or("-"),
            step.map(|step| step.unit_path.as_str()).unwrap_or("-"),
            active.cursor.action_index
        );
        playback_session.restart_at(active.cursor);
        playback_session.state = PlaybackState::Paused {
            position: interrupted_position.expect("active cursor has a position"),
            interruption: PlaybackInterruption {
                reason: PlaybackInterruptionReason::AdapterDisconnected,
                exit_code: None,
                output: None,
                output_truncated: None,
            },
        };
        let _ = playback_tx.send(playback_session.state.clone());
    } else {
        let interrupted_tail = match &playback_session.state {
            PlaybackState::Narrating { position, .. } => playback_session
                .cursor
                .map(|cursor| (cursor, position.clone())),
            PlaybackState::StepComplete {
                position,
                continuation: Some(_),
                ..
            } => Some((
                PlaybackCursor {
                    step_index: position.step_index,
                    action_index: 0,
                },
                position.clone(),
            )),
            _ => None,
        };
        if let Some((current_cursor, position)) = interrupted_tail {
            log::info!(
                target: "cynapse.server",
                "playback.paused correlation_id={} reason=adapter_disconnected step_id={} unit_path={} action_index={}",
                playback_session
                    .correlation_id
                    .as_deref()
                    .unwrap_or("-"),
                position.step_id,
                position.unit_path,
                current_cursor.action_index
            );
            action_narration.clear();
            playback_session.restart_at(current_cursor);
            playback_session.state = PlaybackState::Paused {
                position,
                interruption: PlaybackInterruption {
                    reason: PlaybackInterruptionReason::AdapterDisconnected,
                    exit_code: None,
                    output: None,
                    output_truncated: None,
                },
            };
            let _ = playback_tx.send(playback_session.state.clone());
        }
    }
    log::info!(
        target: "cynapse.server",
        "adapter.disconnected adapter={} generation={generation}",
        plan.adapter_name
    );
    // A replacement tab already owns the connection; its handler must not be
    // marked disconnected by the tab it just replaced.
    if newest.load(std::sync::atomic::Ordering::SeqCst) == generation {
        let _ = state_tx.send(ConnectionState::WaitingForAdapter);
    }
}

/// Resolves once another tab has authenticated; the newest connection wins.
async fn wait_for_replacement(generation_rx: &mut watch::Receiver<u64>, generation: u64) {
    loop {
        if *generation_rx.borrow() != generation {
            return;
        }
        if generation_rx.changed().await.is_err() {
            return;
        }
    }
}

/// Old adapters (released before the `timed_out` field) only sent the code.
pub(super) fn ack_timed_out(payload: &AckPayload) -> bool {
    payload.timed_out == Some(true)
        || (payload.timed_out.is_none() && payload.code.as_deref() == Some("terminal_timeout"))
}

fn interruption_from_payload(
    payload: &AckPayload,
    reason: PlaybackInterruptionReason,
) -> PlaybackInterruption {
    PlaybackInterruption {
        reason,
        exit_code: payload.exit_code,
        output: payload.output.clone(),
        output_truncated: payload.output_truncated,
    }
}

async fn reject(
    socket: &mut WebSocketStream<TcpStream>,
    target_id: &str,
    code: &str,
    message: &str,
    state_tx: &watch::Sender<ConnectionState>,
) {
    let ack = Envelope::ack_error(target_id, code, message);
    if let Ok(ack_json) = ack.to_json() {
        let _ = socket.send(Message::Text(ack_json.into())).await;
    }

    let _ = socket.close(None).await;
    if !has_authenticated_session(state_tx) {
        let _ = state_tx.send(ConnectionState::Error {
            code: code.to_string(),
            message: message.to_string(),
        });
    }
}
