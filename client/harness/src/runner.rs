use super::*;
use cynapse_client_core::unit_session_key;

use super::artifacts::{cleanup_command, repro_command, write_artifacts};
use super::fake_fs::{
    apply_fake_file_system, capture_workspace_contents, copy_directory, list_workspace_files,
    safe_workspace_path,
};

struct Trace {
    started: Instant,
    correlation_id: String,
    events: Vec<ScenarioEvent>,
}

impl Trace {
    fn new(correlation_id: String) -> Self {
        Self {
            started: Instant::now(),
            correlation_id,
            events: Vec::new(),
        }
    }

    fn push(&mut self, kind: ScenarioEventKind, detail: serde_json::Value) {
        self.events.push(ScenarioEvent {
            sequence: self.events.len() as u64 + 1,
            elapsed_ms: self.started.elapsed().as_millis(),
            correlation_id: self.correlation_id.clone(),
            kind,
            detail,
        });
    }
}

pub async fn run_scenario(spec: ScenarioSpec) -> Result<ScenarioReport, ScenarioError> {
    run_scenario_with_setup_and_source(
        spec,
        |_| Ok(()),
        CourseSource::CopyPackage,
        ScenarioOptions::default(),
    )
    .await
}

/// Runs a copied Course Package scenario with execution options.
pub async fn run_scenario_with_options(
    spec: ScenarioSpec,
    options: ScenarioOptions,
) -> Result<ScenarioReport, ScenarioError> {
    run_scenario_with_setup_and_source(spec, |_| Ok(()), CourseSource::CopyPackage, options).await
}

/// Runs a deterministic scenario against the Course Package itself.
///
/// Nothing is copied: the Package is opened where it lives, so scenario writes
/// go to the Package's own workspace directory. Course progress and artifacts
/// still land under `artifact_root`, which is removed afterwards. The options
/// skip work the caller does not need, such as authored `Wait` durations.
pub async fn run_scenario_in_place_with_options(
    spec: ScenarioSpec,
    options: ScenarioOptions,
) -> Result<ScenarioReport, ScenarioError> {
    run_scenario_with_setup_and_source(spec, |_| Ok(()), CourseSource::InPlace, options).await
}

/// Runs a deterministic scenario after preparing any persisted course progress
/// needed by the test, such as a partially completed Unit.
pub async fn run_scenario_with_setup<F>(
    spec: ScenarioSpec,
    setup: F,
) -> Result<ScenarioReport, ScenarioError>
where
    F: FnOnce(&mut CourseSession) -> Result<(), ScenarioError>,
{
    run_scenario_with_setup_and_source(
        spec,
        setup,
        CourseSource::CopyPackage,
        ScenarioOptions::default(),
    )
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CourseSource {
    CopyPackage,
    InPlace,
}

async fn run_scenario_with_setup_and_source<F>(
    spec: ScenarioSpec,
    setup: F,
    source: CourseSource,
    options: ScenarioOptions,
) -> Result<ScenarioReport, ScenarioError>
where
    F: FnOnce(&mut CourseSession) -> Result<(), ScenarioError>,
{
    let correlation_id = Uuid::new_v4().to_string();
    let artifact_path = spec
        .artifact_root
        .join(format!("cynapse-scenario-{correlation_id}"));
    match run_scenario_core(
        spec.clone(),
        correlation_id,
        artifact_path.clone(),
        setup,
        source,
        options,
    )
    .await
    {
        Ok(report) => Ok(report),
        Err(error) => {
            let repro_command = repro_command(&spec);
            let cleanup_command = cleanup_command(&artifact_path);
            fs::create_dir_all(&artifact_path)?;
            fs::write(
                artifact_path.join("failure.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "message": error.to_string(),
                    "artifact_path": artifact_path,
                    "repro_command": repro_command,
                    "cleanup_command": cleanup_command,
                }))?,
            )?;
            Err(ScenarioError::Execution {
                message: error.to_string(),
                artifact_path: artifact_path.to_string_lossy().to_string(),
                repro_command,
                cleanup_command,
            })
        }
    }
}

async fn run_scenario_core(
    spec: ScenarioSpec,
    correlation_id: String,
    artifact_path: PathBuf,
    setup: impl FnOnce(&mut CourseSession) -> Result<(), ScenarioError>,
    source: CourseSource,
    options: ScenarioOptions,
) -> Result<ScenarioReport, ScenarioError> {
    let course_path = match source {
        CourseSource::CopyPackage => {
            let copy = artifact_path.join("course");
            copy_directory(&spec.course_path, &copy)?;
            copy
        }
        CourseSource::InPlace => spec.course_path.clone(),
    };
    let state_db = artifact_path.join("state.db");
    let runtime_path = artifact_path.join("runtime.json");

    let mut session = CourseSession::new(&state_db)?;
    session.open_course(&course_path)?;
    setup(&mut session)?;
    let unit_path = spec
        .unit_path
        .as_deref()
        .map(str::to_string)
        .or_else(|| session.first_unit_path())
        .ok_or_else(|| ScenarioError::Protocol("course has no leaves".to_string()))?;
    session.start_unit(&unit_path, true)?;
    let mut learning = LearningController::default();
    let learning_mode = match spec.mode {
        ScenarioMode::Continuous => cynapse_client_core::LearningMode::Continuous,
        ScenarioMode::ByStep | ScenarioMode::StopAfterStep => {
            cynapse_client_core::LearningMode::ByStep
        }
    };
    learning
        .start(learning_mode, &unit_path)
        .map_err(|error| ScenarioError::Protocol(error.to_string()))?;
    let workspace = session
        .active_workspace()
        .ok_or_else(|| ScenarioError::Protocol("course workspace is not available".to_string()))?;
    fs::create_dir_all(&workspace)?;
    let mut trace = Trace::new(correlation_id.clone());
    trace.push(
        ScenarioEventKind::WorkspacePrepared,
        serde_json::json!({ "path": workspace.clone() }),
    );
    let steps = session
        .try_playback_steps()
        .map_err(|error| ScenarioError::Protocol(error.to_string()))?;
    let resume = session.playback_resume();
    let adapter = session
        .required_adapters_for_unit(&unit_path)
        .first()
        .copied()
        .unwrap_or(AdapterId::Vscode);
    let server = start_server_with_path_and_resume_for_adapter(
        workspace.clone(),
        runtime_path.clone(),
        steps,
        resume,
        AdapterTarget::External(adapter),
    )
    .await?;
    server
        .commands
        .set_skip_unit_final_wait(learning_mode == cynapse_client_core::LearningMode::ByStep)
        .map_err(|error| ScenarioError::Protocol(error.to_string()))?;
    server
        .commands
        .set_skip_waits(options.skip_waits)
        .map_err(|error| ScenarioError::Protocol(error.to_string()))?;

    trace.push(
        ScenarioEventKind::ScenarioStarted,
        serde_json::json!({
            "course_path": spec.course_path,
            "unit_path": unit_path,
            "mode": spec.mode,
            "workspace": workspace.clone(),
        }),
    );

    let mut adapter_messages = Vec::new();
    let result = execute_adapter(
        &server,
        &workspace,
        &mut trace,
        &mut adapter_messages,
        &spec,
        options,
        &mut session,
        &mut learning,
    )
    .await;
    let playback_state = server.playback.borrow().clone();
    let course_state = session.state();
    let workspace_files = list_workspace_files(&workspace)?;
    let (workspace_contents, workspace_contents_skipped) =
        capture_workspace_contents(&workspace, &workspace_files);
    trace.push(
        ScenarioEventKind::WorkspaceFilesCaptured,
        serde_json::json!({ "files": workspace_files.clone() }),
    );
    let status = match result {
        Ok(status) => status,
        Err(error) => {
            trace.push(
                ScenarioEventKind::ScenarioError,
                serde_json::json!({ "message": error.to_string() }),
            );
            drop(server);
            return Err(error);
        }
    };
    let artifacts_retained = status != ScenarioStatus::Completed;
    let report = ScenarioReport {
        correlation_id,
        status,
        course_state,
        playback_state,
        events: trace.events,
        adapter_messages,
        workspace_files,
        workspace_contents,
        workspace_contents_skipped,
        artifact_path: artifact_path.to_string_lossy().to_string(),
        logs_path: artifact_path
            .join("events.jsonl")
            .to_string_lossy()
            .to_string(),
        artifacts_retained,
        repro_command: repro_command(&spec),
        cleanup_command: cleanup_command(&artifact_path),
    };
    write_artifacts(&artifact_path, &report)?;
    server.shutdown().await;
    // Close the state database before deleting its artifact directory; Windows
    // keeps the file locked while the CourseSession connection is open.
    drop(session);
    if !artifacts_retained {
        fs::remove_dir_all(&artifact_path)?;
    }
    Ok(report)
}

struct PendingChallenge {
    challenge: cynapse_course::Challenge,
    attempt: usize,
    position: PlaybackPosition,
}

fn send_scenario_check(
    server: &ServerHandle,
    challenge: &cynapse_course::Challenge,
    unit_path: &str,
    attempt: usize,
    response_timeout: Duration,
    result_tx: &tokio::sync::mpsc::UnboundedSender<Result<AckPayload, String>>,
    trace: &mut Trace,
) -> Result<(), ScenarioError> {
    let Some(check) = challenge.checks.first() else {
        return Err(ScenarioError::Protocol(
            "challenge has no check".to_string(),
        ));
    };
    let payload = CheckPayload {
        kind: "COMMAND".to_string(),
        unit_path: unit_path.to_string(),
        session_key: unit_session_key(unit_path, check.session_key()),
        cwd: check.cwd().to_string(),
        command: check.command().to_string(),
        accepted_exit_codes: check.accepted_exit_codes().to_vec(),
        expected_output: check.expected_output().to_vec(),
        timeout_ms: check.timeout_ms(),
        save_dirty: false,
    };
    let receiver = server
        .commands
        .execute_check(payload)
        .map_err(|_| ScenarioError::Protocol("challenge check is unavailable".to_string()))?;
    trace.push(
        ScenarioEventKind::AdapterMessageSent,
        serde_json::json!({
            "kind": "check",
            "attempt": attempt,
        }),
    );
    let result_tx = result_tx.clone();
    tokio::spawn(async move {
        let result = match tokio::time::timeout(response_timeout, receiver).await {
            Ok(Ok(Ok(ack))) => Ok(ack),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err("challenge check result channel closed".to_string()),
            Err(_) => Err("challenge check timed out".to_string()),
        };
        let _ = result_tx.send(result);
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_adapter(
    server: &ServerHandle,
    workspace: &Path,
    trace: &mut Trace,
    adapter_messages: &mut Vec<AdapterMessage>,
    spec: &ScenarioSpec,
    options: ScenarioOptions,
    session: &mut CourseSession,
    learning: &mut LearningController,
) -> Result<ScenarioStatus, ScenarioError> {
    let discovery: DiscoveryFile = serde_json::from_slice(&fs::read(&server.runtime_path)?)?;
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/", discovery.port)).await?;
    socket
        .send(Message::Text(
            Envelope::hello(discovery.token).to_json()?.into(),
        ))
        .await?;
    let hello_ack = receive_envelope(&mut socket).await?;
    trace.push(
        ScenarioEventKind::AdapterConnected,
        serde_json::json!({ "adapter": hello_ack.payload }),
    );
    play(&server.commands, &trace.correlation_id)?;

    let mut playback = server.playback.clone();
    let mut handled_narration = None;
    let mut pending_narration_end = None;
    let mut pending_terminal_cleanup = None;
    let mut stop_requested = false;
    let mut narration_retry_sent = false;
    let mut action_index = 0usize;
    let mut check_index = 0usize;
    let (challenge_tx, mut challenge_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut pending_challenge: Option<PendingChallenge> = None;
    loop {
        tokio::select! {
            changed = playback.changed() => {
                if changed.is_err() {
                    return Ok(ScenarioStatus::PlaybackError);
                }
                let state = playback.borrow().clone();
                debug_event(format!("playback {}", serde_json::to_string(&state).unwrap()));
                trace.push(
                    ScenarioEventKind::PlaybackState,
                    serde_json::to_value(&state).unwrap(),
                );
                let learning_view = learning.observe(&state);
                trace.push(
                    ScenarioEventKind::LearningState,
                    serde_json::to_value(&learning_view).unwrap(),
                );
                match state {
                    PlaybackState::StepComplete {
                        position,
                        requires_human,
                        continuation,
                    } => {
                        if let Some(challenge) =
                            session.challenge(&position.unit_path, &position.step_id)
                        {
                            send_scenario_check(
                                server,
                                &challenge,
                                &position.unit_path,
                                0,
                                spec.response_timeout,
                                &challenge_tx,
                                trace,
                            )?;
                            pending_challenge = Some(PendingChallenge {
                                challenge,
                                attempt: 0,
                                position: position.clone(),
                            });
                            continue;
                        }
                        session.record_progress(
                            &position.unit_path,
                            &position.step_id,
                            true,
                        )?;
                        if requires_human {
                            trace.push(
                                ScenarioEventKind::HumanStepReached,
                                serde_json::json!({
                                    "unit_path": position.unit_path,
                                    "step_id": position.step_id,
                                }),
                            );
                            return Ok(ScenarioStatus::WaitingForHuman);
                        } else if spec.mode == ScenarioMode::StopAfterStep && !stop_requested {
                            stop_requested = true;
                            if let Some(path) =
                                session.current_unit_workspace_relative_path()
                            {
                                pending_terminal_cleanup = Some(
                                    server
                                        .commands
                                        .terminal_cleanup(vec![path])
                                        .map_err(|_| {
                                            ScenarioError::Protocol(
                                                "terminal cleanup is unavailable".to_string(),
                                            )
                                        })?,
                                );
                            } else {
                                let stopped = learning.stop();
                                session.clear_selection()?;
                                trace.push(
                                    ScenarioEventKind::StopCompleted,
                                    serde_json::to_value(&stopped).unwrap(),
                                );
                                return Ok(ScenarioStatus::Stopped);
                            }
                        } else if continuation.is_none() {
                            play(&server.commands, &trace.correlation_id)?;
                        }
                    }
                    PlaybackState::UnitComplete { position } => {
                        if spec.mode == ScenarioMode::Continuous
                            && human_unit_pause(session, &position)?
                        {
                            session.record_progress(
                                &position.unit_path,
                                &position.step_id,
                                true,
                            )?;
                            trace.push(
                                ScenarioEventKind::HumanStepReached,
                                serde_json::json!({
                                    "unit_path": position.unit_path,
                                    "step_id": position.step_id,
                                }),
                            );
                            return Ok(ScenarioStatus::WaitingForHuman);
                        }
                        if !handle_unit_boundary(
                            server,
                            session,
                            learning,
                            trace,
                            &position,
                            spec.mode,
                            spec.response_timeout,
                        )
                        .await?
                        {
                            return Ok(ScenarioStatus::Completed);
                        }
                    }
                    PlaybackState::Narrating { clip, position, .. } => {
                        let key = clip.clone();
                        if handled_narration.as_ref() != Some(&key) {
                            match spec.adapter_script.narration {
                                NarrationReply::Complete => {}
                                NarrationReply::EndAfterActionAck => {
                                    server.commands.narration_started(clip.clone())
                                        .map_err(|_| ScenarioError::Protocol("narration start failed".to_string()))?;
                                    pending_narration_end =
                                        Some((clip.clone(), position.step_id.clone()));
                                    handled_narration = Some(key);
                                    continue;
                                }
                                NarrationReply::Disconnect => {
                                    trace.push(
                                        ScenarioEventKind::AdapterDisconnected,
                                        serde_json::json!({
                                            "clip": clip,
                                        }),
                                    );
                                    return Ok(ScenarioStatus::AdapterDisconnected);
                                }
                                NarrationReply::Timeout => {
                                    tokio::time::sleep(spec.response_timeout).await;
                                    trace.push(
                                        ScenarioEventKind::AdapterTimeout,
                                        serde_json::json!({
                                            "timeout_ms": spec.response_timeout.as_millis(),
                                            "clip": clip,
                                        }),
                                    );
                                    return Ok(ScenarioStatus::TimedOut);
                                }
                                reply
                                    if reply == NarrationReply::Fail
                                        || (reply == NarrationReply::FailOnce
                                            && !narration_retry_sent) =>
                                {
                                    narration_retry_sent = true;
                                    server
                                        .commands
                                        .narration_failed(
                                            clip.clone(),
                                            "scripted media failure",
                                        )
                                        .map_err(|_| {
                                            ScenarioError::Protocol(
                                                "narration failure could not be reported"
                                                    .to_string(),
                                            )
                                        })?;
                                    handled_narration = Some(key);
                                    continue;
                                }
                                NarrationReply::FailOnce => {}
                                NarrationReply::Fail => unreachable!("Fail handled above"),
                            }
                            server.commands.narration_started(clip.clone())
                                .map_err(|_| ScenarioError::Protocol("narration start failed".to_string()))?;
                            server.commands.narration_ended(clip.clone())
                                .map_err(|_| ScenarioError::Protocol("narration end failed".to_string()))?;
                            trace.push(ScenarioEventKind::NarrationCompleted, serde_json::json!({
                                "clip": clip,
                                "step_id": position.step_id,
                            }));
                            handled_narration = Some(key);
                        }
                    }
                    PlaybackState::Error {
                        code,
                        message,
                        position,
                        ..
                    } => {
                        trace.push(ScenarioEventKind::PlaybackError, serde_json::json!({
                            "code": code,
                            "message": message,
                            "position": position,
                        }));
                        if code == "narration_failed"
                            && spec.adapter_script.narration
                                == NarrationReply::FailOnce
                            && narration_retry_sent
                        {
                            play(&server.commands, &trace.correlation_id)?;
                            continue;
                        }
                        return Ok(ScenarioStatus::PlaybackError);
                    }
                    PlaybackState::Paused { interruption, .. } => {
                        trace.push(
                            ScenarioEventKind::PlaybackPaused,
                            serde_json::to_value(&interruption).unwrap(),
                        );
                        if matches!(
                            interruption.reason,
                            PlaybackInterruptionReason::UserStop
                                | PlaybackInterruptionReason::JupyterLabKernelRestarted
                        ) {
                            return Ok(ScenarioStatus::Cancelled);
                        }
                    }
                    _ => {}
                }
            }
            result = challenge_rx.recv(), if pending_challenge.is_some() => {
                let pending = pending_challenge
                    .take()
                    .expect("pending challenge should exist");
                let ack = match result {
                    Some(Ok(ack)) => ack,
                    Some(Err(error)) => return Err(ScenarioError::Protocol(error)),
                    None => {
                        return Err(ScenarioError::Protocol(
                            "challenge check result channel closed".to_string(),
                        ));
                    }
                };
                trace.push(
                    ScenarioEventKind::AdapterMessageSent,
                    serde_json::json!({
                        "kind": "check",
                        "attempt": pending.attempt,
                        "status": ack.status,
                        "exit_code": ack.exit_code,
                        "code": ack.code,
                    }),
                );
                if ack.status == AckStatus::Done {
                    session.record_challenge_pass(
                        &pending.position.unit_path,
                        &pending.position.step_id,
                    )?;
                    play(&server.commands, &trace.correlation_id)?;
                } else if pending.attempt == 0 && options.skip_challenge {
                    if !pending.challenge.allow_skip {
                        return Ok(ScenarioStatus::WaitingForHuman);
                    }
                    session.record_challenge_failure(
                        &pending.position.unit_path,
                        &pending.position.step_id,
                    )?;
                    session.record_challenge_skip(
                        &pending.position.unit_path,
                        &pending.position.step_id,
                    )?;
                    play(&server.commands, &trace.correlation_id)?;
                } else if pending.attempt == 0 {
                    session.record_challenge_failure(
                        &pending.position.unit_path,
                        &pending.position.step_id,
                    )?;
                    let solution = spec.course_path.join("solution");
                    if solution.is_dir() {
                        let workspace_root = session
                            .active_workspace()
                            .unwrap_or_else(|| workspace.to_path_buf());
                        let unit_workspace = safe_workspace_path(
                            &workspace_root,
                            &pending.position.unit_path,
                        )?;
                        copy_directory(&solution, &unit_workspace)?;
                    }
                    send_scenario_check(
                        server,
                        &pending.challenge,
                        &pending.position.unit_path,
                        1,
                        spec.response_timeout,
                        &challenge_tx,
                        trace,
                    )?;
                    pending_challenge = Some(PendingChallenge {
                        challenge: pending.challenge,
                        attempt: 1,
                        position: pending.position,
                    });
                } else {
                    session.record_challenge_failure(
                        &pending.position.unit_path,
                        &pending.position.step_id,
                    )?;
                    return Ok(ScenarioStatus::PlaybackError);
                }
            }
            message = socket.next() => {
                let Some(message) = message else {
                    trace.push(ScenarioEventKind::AdapterDisconnected, serde_json::json!({}));
                    return Ok(ScenarioStatus::AdapterDisconnected);
                };
                let message = message?;
                let Message::Text(text) = message else {
                    continue;
                };
                let envelope = Envelope::parse(text.as_str())?;
                debug_event(format!(
                    "adapter received {:?} {}",
                    envelope.kind,
                    envelope.id
                ));
                let received_kind = if envelope.kind == MessageType::Action {
                    ScenarioEventKind::ActionReceived
                } else {
                    ScenarioEventKind::AdapterMessageReceived
                };
                trace.push(received_kind, serde_json::to_value(&envelope).unwrap());
                adapter_messages.push(AdapterMessage {
                    correlation_id: trace.correlation_id.clone(),
                    direction: AdapterDirection::Received,
                    envelope: envelope.clone(),
                });
                let mut action_to_apply = None;
                let ack_id = if envelope.kind == MessageType::Cancel {
                    envelope.payload["target_id"]
                        .as_str()
                        .unwrap_or(&envelope.id)
                        .to_string()
                } else {
                    envelope.id.clone()
                };
                let reply = match envelope.kind {
                    MessageType::Action => {
                        if let Ok(action) =
                            serde_json::from_value::<Action>(envelope.payload.clone())
                        {
                            action_to_apply = Some(action);
                        }
                        let reply = spec.adapter_script.reply_for(action_index);
                        action_index += 1;
                        reply
                    }
                    MessageType::WorkspaceReset | MessageType::Cancel => {
                        AdapterReply::Acknowledge {
                            payload: cancelled_or_successful_ack(envelope.kind),
                        }
                    }
                    MessageType::Check => {
                        let reply = spec
                            .adapter_script
                            .check_replies
                            .get(check_index)
                            .cloned()
                            .unwrap_or(AdapterReply::Acknowledge {
                                payload: successful_ack(),
                            });
                        check_index += 1;
                        reply
                    }
                    _ => continue,
                };
                let action_done = envelope.kind == MessageType::Action
                    && matches!(
                        &reply,
                        AdapterReply::Acknowledge { payload }
                            if payload.status == AckStatus::Done
                    );
                match reply {
                    AdapterReply::Acknowledge { payload } => {
                        if payload.status == AckStatus::Done
                            && let Some(action) = action_to_apply.as_ref()
                        {
                            apply_fake_file_system(workspace, action)?;
                        }
                        let ack = Envelope {
                            v: 1,
                            id: ack_id,
                            kind: MessageType::Ack,
                            unit_path: None,
                            payload: serde_json::to_value(payload)?,
                        };
                        trace.push(
                            ScenarioEventKind::AdapterMessageSent,
                            serde_json::to_value(&ack).unwrap(),
                        );
                        adapter_messages.push(AdapterMessage {
                            correlation_id: trace.correlation_id.clone(),
                            direction: AdapterDirection::Sent,
                            envelope: ack.clone(),
                        });
                        socket.send(Message::Text(ack.to_json()?.into())).await?;
                        if action_done && let Some((clip, step_id)) = pending_narration_end.take()
                        {
                            server.commands.narration_ended(clip.clone()).map_err(|_| {
                                ScenarioError::Protocol("narration end failed".to_string())
                            })?;
                            trace.push(
                                ScenarioEventKind::NarrationCompleted,
                                serde_json::json!({
                                    "clip": clip,
                                    "step_id": step_id,
                                }),
                            );
                        }
                        if envelope.kind == MessageType::WorkspaceReset
                            && let Some(completion) = pending_terminal_cleanup.take()
                        {
                                tokio::time::timeout(
                                    spec.response_timeout,
                                    completion,
                                )
                                .await
                                .map_err(|_| {
                                    ScenarioError::Protocol(
                                        "terminal cleanup timed out".to_string(),
                                    )
                                })?
                                .map_err(|_| {
                                    ScenarioError::Protocol(
                                        "terminal cleanup completion channel closed"
                                            .to_string(),
                                    )
                                })?
                                .map_err(ScenarioError::Protocol)?;
                                let stopped = learning.stop();
                                let unit_path = session
                                    .state()
                                    .selected_unit
                                    .as_ref()
                                    .map(|unit| unit.path.clone());
                                if let Some(unit_path) = unit_path {
                                    session.select_unit(&unit_path)?;
                                    let resume = session.playback_resume();
                                    let resume_detail = resume.as_ref().map(|resume| {
                                        serde_json::json!({
                                            "unit_path": resume.unit_path,
                                            "step_id": resume.step_id,
                                            "step_started": resume.step_started,
                                        })
                                    });
                                    trace.push(
                                        ScenarioEventKind::ResumeCaptured,
                                        serde_json::to_value(resume_detail).unwrap(),
                                    );
                                }
                                session.clear_selection()?;
                                trace.push(
                                    ScenarioEventKind::StopCompleted,
                                    serde_json::to_value(&stopped).unwrap(),
                                );
                                return Ok(ScenarioStatus::Stopped);
                        }
                    }
                    AdapterReply::Disconnect => return Ok(ScenarioStatus::AdapterDisconnected),
                    AdapterReply::Cancel => {
                        debug_event("scenario requested cancellation".to_string());
                        server
                            .commands
                            .cancel_correlated(CancelReason::UserStop, &trace.correlation_id)
                            .map_err(|_| {
                                ScenarioError::Protocol(
                                    "playback cancellation is unavailable".to_string(),
                                )
                            })?;
                    }
                    AdapterReply::Timeout => {
                        tokio::time::sleep(spec.response_timeout).await;
                        trace.push(
                            ScenarioEventKind::AdapterTimeout,
                            serde_json::json!({
                                "timeout_ms": spec.response_timeout.as_millis(),
                                "action_id": envelope.id,
                            }),
                        );
                        return Ok(ScenarioStatus::TimedOut);
                    }
                }
            }
        }
    }
}

fn human_unit_pause(
    session: &CourseSession,
    position: &PlaybackPosition,
) -> Result<bool, ScenarioError> {
    let is_human = session
        .try_playback_steps()
        .map_err(|error| ScenarioError::Protocol(error.to_string()))?
        .iter()
        .any(|step| {
            step.unit_path == position.unit_path
                && step.step_id == position.step_id
                && step.requires_human
                && !step.challenge
        });
    if !is_human {
        return Ok(false);
    }
    Ok(session
        .continuous_transition_after(&position.unit_path)?
        .is_some())
}

#[allow(clippy::too_many_arguments)]
async fn handle_unit_boundary(
    server: &ServerHandle,
    session: &mut CourseSession,
    learning: &mut LearningController,
    trace: &mut Trace,
    position: &PlaybackPosition,
    mode: ScenarioMode,
    response_timeout: Duration,
) -> Result<bool, ScenarioError> {
    session.record_progress(&position.unit_path, &position.step_id, true)?;
    if mode == ScenarioMode::Continuous {
        advance_continuous_unit(server, session, learning, trace, position, response_timeout).await
    } else {
        Ok(false)
    }
}

async fn advance_continuous_unit(
    server: &ServerHandle,
    session: &mut CourseSession,
    learning: &mut LearningController,
    trace: &mut Trace,
    completed: &PlaybackPosition,
    response_timeout: Duration,
) -> Result<bool, ScenarioError> {
    let Some(plan) = session.continuous_transition_after(&completed.unit_path)? else {
        if session
            .state()
            .active_course
            .and_then(|course| course.current_unit_path)
            .is_some()
        {
            session.clear_selection()?;
        }
        let completed_view = learning.complete_unit();
        trace.push(
            ScenarioEventKind::LearningState,
            serde_json::to_value(completed_view).unwrap(),
        );
        return Ok(false);
    };

    if plan.reset_required {
        trace.push(
            ScenarioEventKind::ContinuousUnitReset,
            serde_json::json!({
                "unit_path": plan.location.path,
                "workspace_paths": plan.reset_workspace_paths.clone(),
            }),
        );
    }
    let playback = session.start_continuous_transition(&plan)?;
    let replacement = server
        .commands
        .replace_plan(playback.steps, playback.resume)
        .map_err(|_| ScenarioError::Protocol("playback plan replacement failed".to_string()))?;
    tokio::time::timeout(response_timeout, replacement)
        .await
        .map_err(|_| ScenarioError::Protocol("playback plan replacement timed out".to_string()))?
        .map_err(|_| {
            ScenarioError::Protocol("playback plan replacement completion closed".to_string())
        })?
        .map_err(ScenarioError::Protocol)?;
    let learning_view = learning.move_to_unit(&plan.location.path);
    trace.push(
        ScenarioEventKind::LearningState,
        serde_json::to_value(learning_view).unwrap(),
    );
    trace.push(
        ScenarioEventKind::ContinuousUnitAdvanced,
        serde_json::json!({
            "from": {
                "unit_path": completed.unit_path,
            },
            "to": {
                "unit_path": plan.location.path,
            },
        }),
    );
    play(&server.commands, &trace.correlation_id)?;
    Ok(true)
}

fn play(
    commands: &cynapse_client_core::server::PlaybackCommandSender,
    correlation_id: &str,
) -> Result<(), ScenarioError> {
    commands
        .play_correlated(correlation_id)
        .map_err(|_| ScenarioError::Protocol("playback server is unavailable".to_string()))
}

fn debug_event(message: String) {
    if std::env::var_os("CYNAPSE_SCENARIO_DEBUG").is_some() {
        eprintln!("[scenario] {message}");
    }
}

async fn receive_envelope(socket: &mut ClientSocket) -> Result<Envelope, ScenarioError> {
    let message = socket
        .next()
        .await
        .ok_or_else(|| ScenarioError::Protocol("adapter socket closed".to_string()))??;
    let Message::Text(text) = message else {
        return Err(ScenarioError::Protocol(
            "adapter received a non-text protocol message".to_string(),
        ));
    };
    Ok(Envelope::parse(text.as_str())?)
}

pub(super) fn successful_ack() -> AckPayload {
    AckPayload {
        status: AckStatus::Done,
        code: None,
        message: None,
        exit_code: Some(0),
        output: None,
        output_truncated: Some(false),
        completion_marker_found: None,
        timed_out: None,
    }
}

fn cancelled_or_successful_ack(kind: MessageType) -> AckPayload {
    let mut payload = successful_ack();
    if kind == MessageType::Cancel {
        payload.status = AckStatus::Cancelled;
        payload.exit_code = None;
    }
    payload
}
