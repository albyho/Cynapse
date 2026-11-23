//! Learning start/stop, progression, and Challenge use cases.

use super::*;

impl SessionService {
    /// Shared gate for actions that need the Unit's effective Adapters.
    pub fn ensure_adapter_ready(&self, unit_path: &str) -> Result<(), String> {
        let adapters = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?
            .required_adapters_for_unit(unit_path);
        check_adapter_readiness(&adapters, &self.connection_state())
            .map_err(|warning| warning.text())
    }

    pub(super) async fn check_unit_environment(
        &self,
        unit_path: &str,
        environment: &[EnvironmentRequirement],
        ticket: &ClientTransitionTicket,
    ) -> Result<(), String> {
        if environment.is_empty() {
            return Ok(());
        }
        self.wait_for_connection_or_cancel(unit_path, ticket)
            .await?;
        let report = self
            .server_commands()?
            .check_environment(
                environment,
                unit_path,
                cynapse_client_core::server::ENVIRONMENT_CHECK_SESSION_KEY,
                ".",
            )
            .await;
        if report.ok {
            return Ok(());
        }
        let failures = report
            .checks
            .iter()
            .filter(|check| !check.ok)
            .map(|check| check.message.clone())
            .collect::<Vec<_>>()
            .join(&t("list.separator"));
        Err(t_with(
            "error.unit_preflight_failed",
            &[("failures", &failures)],
        ))
    }

    pub(super) fn ensure_active_unit_adapter_ready(&self) -> Result<(), String> {
        let unit_path = self
            .playback_state()
            .ok()
            .and_then(|playback| {
                playback_position(&playback).map(|position| position.unit_path.clone())
            })
            .or_else(|| {
                self.course_state()
                    .ok()
                    .and_then(|state| state.selected_unit.map(|unit| unit.path))
            });
        match unit_path {
            Some(unit_path) => self.ensure_adapter_ready(&unit_path),
            None => Ok(()),
        }
    }

    pub async fn begin_learning(&self, mode: LearningMode, unit_path: &str) -> Result<(), String> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Starting,
            ClientTransitionRequirement::AdapterStarting,
            t("transition.starting"),
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            if !self.inner.transitions.transition_is_current(&ticket) {
                return Err(t("error.learning_start_cancelled"));
            }
            let _guard = self.inner._runtime.enter();
            self.update_starting_stage(
                ClientTransitionStage::PrepareEnvironment,
                t("startup.step.environment"),
            );
            self.prepare_jupyterlab(unit_path).await?;
            self.ensure_protocol_adapter(unit_path, &ticket).await?;
            self.update_starting_stage(
                ClientTransitionStage::ConnectAdapter,
                t("startup.step.connection"),
            );
            self.ensure_adapter_ready(unit_path)?;
            let environment = self
                .inner
                .session
                .lock()
                .map(|session| session.environment_requirements_for_unit(unit_path))
                .unwrap_or_default();
            if !environment.is_empty() {
                self.update_starting_stage(
                    ClientTransitionStage::CheckEnvironment,
                    t("startup.step.check"),
                );
                self.check_unit_environment(unit_path, &environment, &ticket)
                    .await?;
            }

            self.update_starting_stage(
                ClientTransitionStage::PrepareWorkspace,
                t("startup.step.workspace"),
            );
            let (steps, resume, adapter, needs_adapter, state) = {
                let mut session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                let state = session
                    .start_unit(unit_path, true)
                    .map_err(|error| error.to_string())?;
                let adapters = session.required_adapters_for_unit(unit_path);
                (
                    session
                        .try_playback_steps()
                        .map_err(|error| error.to_string())?,
                    session.playback_resume(),
                    adapters.first().copied().unwrap_or(AdapterId::Vscode),
                    !adapters.is_empty(),
                    state,
                )
            };
            let _ = self.inner.events.send(ClientEvent::Course(Arc::new(state)));

            // The server and its Adapter connection survive the Start. Wait for
            // an actual reconnect event instead of a fixed grace period.
            let connected = matches!(self.connection_state(), ConnectionState::Connected { .. });
            let commands = if !needs_adapter && !connected {
                // ReplacePlan runs in the Adapter connection loop; a Unit
                // without an Adapter needs a fresh server with its plan.
                if self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?
                    .active_workspace()
                    .is_none()
                {
                    return Err(t("error.course_workspace_unavailable"));
                }
                self.replace_server_for_adapter(adapter, ReplacementKind::Controlled, None)
                    .await?;
                self.server_commands()?
            } else {
                self.wait_for_connection_or_cancel(unit_path, &ticket)
                    .await?;
                let commands = self.server_commands()?;
                let replacement = commands
                    .replace_plan(steps, resume)
                    .map_err(|error| error.to_string())?;
                let replaced = tokio::select! {
                    result = replacement => result,
                    _ = self.inner.transitions.wait_for_transition_end(&ticket) => {
                        return Err(t("error.learning_start_cancelled"));
                    }
                };
                if !matches!(replaced, Ok(Ok(()))) {
                    return Err(t("error.playback_plan_unavailable"));
                }
                commands
            };
            commands
                .set_skip_unit_final_wait(mode == LearningMode::ByStep)
                .map_err(|error| error.to_string())?;
            if !self.inner.transitions.transition_is_current(&ticket) {
                return Err(t("error.learning_start_cancelled"));
            }
            self.update_starting_stage(
                ClientTransitionStage::StartPlayback,
                t("startup.step.playback"),
            );
            commands.play().map_err(|error| error.to_string())?;
            Ok::<_, String>(())
        }
        .await;

        self.finish_transition(ticket);
        result
    }

    pub async fn stop_learning(&self) -> Result<(), String> {
        let stop_ticket = self.begin_stop_transition();
        let unit_path = self
            .inner
            .session
            .lock()
            .ok()
            .and_then(|session| session.current_unit_workspace_relative_path());
        let _guard = self.inner._runtime.enter();
        let result = self.interrupt_playback(CancelReason::UserStop).await;
        if result.is_ok()
            && let Some(path) = unit_path
            && let Err(error) = self.cleanup_terminals(vec![path]).await
        {
            let _ = self
                .inner
                .events
                .send(ClientEvent::Warnings(vec![error.into()]));
        }
        if let Some(ticket) = stop_ticket {
            self.finish_stop_transition(ticket);
        }
        result
    }

    /// Re-dispatches the interrupted Action from the server's current cursor.
    pub fn retry_playback(&self) -> Result<(), String> {
        self.ensure_active_unit_adapter_ready()?;
        let playback = self.playback_state()?;
        if !playback_is_retryable(&playback) {
            return Err(t("error.no_retry_position"));
        }
        let ticket =
            self.try_start_transition(ClientTransitionKind::Retrying, t("transition.retrying"))?;
        let result = self
            .server_commands()?
            .play()
            .map_err(|error| error.to_string());
        self.finish_transition(ticket);
        result
    }

    /// Advances past a completed Step once any Challenge on it has been passed
    /// or skipped. The server owns the next cursor; the caller only presses
    /// the explicit boundary.
    pub fn advance_step(&self) -> Result<(), String> {
        self.ensure_active_unit_adapter_ready()?;
        let ticket =
            self.try_start_transition(ClientTransitionKind::Advancing, t("transition.advancing"))?;
        let result = (|| -> Result<(), String> {
            let playback = self.playback_state()?;
            let PlaybackState::StepComplete { position, .. } = &playback else {
                return Err(t("error.step_not_at_boundary"));
            };
            {
                let session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                if session
                    .challenge(&position.unit_path, &position.step_id)
                    .is_some()
                {
                    let status = session
                        .challenge_status(&position.unit_path, &position.step_id)
                        .unwrap_or(ChallengeStatus::NotStarted);
                    if !matches!(status, ChallengeStatus::Passed | ChallengeStatus::Skipped) {
                        return Err(t("error.pass_or_skip_challenge"));
                    }
                }
            }
            self.server_commands()?
                .play()
                .map_err(|error| error.to_string())
        })();
        self.finish_transition(ticket);
        result
    }

    /// Starts the next Unit in Course Tree order after a completed Unit. A
    /// previously touched Unit is reset first, matching Continuous Learning's
    /// unit transition contract.
    pub async fn advance_unit(
        &self,
        mode: LearningMode,
    ) -> Result<CourseSessionState, cynapse_client_core::Warning> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Advancing,
            ClientTransitionRequirement::AdapterIndependent,
            t("transition.advancing_unit"),
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            let _guard = self.inner._runtime.enter();
            let playback = self.playback_state()?;
            let PlaybackState::UnitComplete { position } = &playback else {
                return Err(t("error.unit_not_complete").into());
            };
            let plan = {
                let session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                session
                    .continuous_transition_after(&position.unit_path)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| t("error.no_next_unit"))?
            };
            self.prepare_jupyterlab(&plan.location.path).await?;
            self.ensure_protocol_adapter(&plan.location.path, &ticket)
                .await?;
            check_adapter_readiness(&plan.adapters, &self.connection_state())?;
            self.check_unit_environment(
                &plan.location.path,
                &plan.environment_requirements,
                &ticket,
            )
            .await?;
            if plan.reset_required {
                if !self.begin_finalizing_transition(&ticket) {
                    return Err(t("error.next_unit_superseded").into());
                }
                self.cleanup_workspace(
                    plan.reset_workspace_paths.clone(),
                    plan.reset_exclude_paths.clone(),
                )
                .await?;
            }
            let (state, playback_plan) = self
                .inner
                .transitions
                .commit_transition(&ticket, || {
                    let mut session = self
                        .inner
                        .session
                        .lock()
                        .map_err(|_| t("error.course_session_unavailable"))?;
                    let playback_plan = session
                        .start_continuous_transition(&plan)
                        .map_err(|error| error.to_string())?;
                    Ok((session.state(), playback_plan))
                })
                .map_err(transition_commit_error)?;
            self.publish_course_state(state.clone());
            let commands = self.server_commands()?;
            let replacement = commands
                .replace_plan(playback_plan.steps, playback_plan.resume)
                .map_err(|error| error.to_string())?;
            let replaced = tokio::time::timeout(NEXT_UNIT_PLAN_TIMEOUT, replacement).await;
            if !matches!(replaced, Ok(Ok(Ok(())))) {
                self.reset_playback_plan().await;
                return Err(cynapse_client_core::Warning::NextUnitPlanUnavailable);
            }
            commands
                .set_skip_unit_final_wait(mode == LearningMode::ByStep)
                .map_err(|error| error.to_string())?;
            commands.play().map_err(|error| error.to_string())?;
            Ok(state)
        }
        .await;
        self.finish_transition(ticket);
        result
    }

    pub async fn check_challenge(
        &self,
        unit_path: &str,
        step_id: &str,
        save_dirty: bool,
    ) -> Result<ChallengeCheckResult, String> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Preparing,
            ClientTransitionRequirement::AdapterDependent,
            t("transition.checking_challenge"),
        )?;
        let result = async {
            let _guard = self.inner._runtime.enter();
            self.wait_for_transition(&ticket).await?;
            self.playback_state()
                .and_then(|playback| ensure_checkpoint(&playback, unit_path, step_id))?;
            self.ensure_adapter_ready(unit_path)?;
            let check = {
                let session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                session
                    .challenge(unit_path, step_id)
                    .and_then(|challenge| challenge.checks.first().cloned())
                    .ok_or_else(|| t("error.no_checkable_challenge"))?
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
                save_dirty,
            };
            let commands = self.server_commands()?;
            let receiver = commands
                .execute_check(payload)
                .map_err(|error| error.to_string())?;
            let timeout =
                Duration::from_millis(check.timeout_ms()).saturating_add(CHECK_TIMEOUT_GRACE);
            let ack = match tokio::time::timeout(timeout, receiver).await {
                Ok(result) => result
                    .map_err(|_| t("error.challenge_check_interrupted"))?
                    .map_err(|error| {
                        let detail = error.to_string();
                        t_with("error.challenge_check_failed", &[("error", &detail)])
                    })?,
                Err(_) => {
                    let _ = commands.cancel_pending_one_off(CancelReason::UserStop);
                    return Err(t("error.challenge_check_timeout"));
                }
            };

            if self
                .playback_state()
                .and_then(|playback| ensure_checkpoint(&playback, unit_path, step_id))
                .is_err()
                || !self.inner.transitions.transition_is_current(&ticket)
            {
                let mut result = ChallengeCheckResult::new(ChallengeOutcome::Cancelled);
                result.output = ack.output;
                result.message = Some(t("error.challenge_left_checkpoint"));
                result.exit_code = ack.exit_code;
                result.output_truncated = ack.output_truncated.unwrap_or(false);
                return Ok(result);
            }

            let outcome = if ack.timed_out == Some(true) || is_check_failure(&ack) {
                self.record_challenge(unit_path, step_id, ChallengeOutcome::Failed)?;
                ChallengeOutcome::Failed
            } else {
                match ack.status {
                    AckStatus::Done => {
                        self.record_challenge(unit_path, step_id, ChallengeOutcome::Passed)?;
                        ChallengeOutcome::Passed
                    }
                    AckStatus::Error if ack.code.as_deref() == Some("workspace_dirty") => {
                        ChallengeOutcome::WorkspaceDirty
                    }
                    AckStatus::Error => ChallengeOutcome::Error,
                    AckStatus::Cancelled => ChallengeOutcome::Cancelled,
                }
            };
            let mut result = ChallengeCheckResult::new(outcome);
            result.output = ack.output;
            result.message = ack.message;
            result.exit_code = ack.exit_code;
            result.output_truncated = ack.output_truncated.unwrap_or(false);
            Ok(result)
        }
        .await;
        self.finish_transition(ticket);
        result
    }

    pub async fn skip_challenge(
        &self,
        unit_path: &str,
        step_id: &str,
    ) -> Result<ChallengeCheckResult, String> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Preparing,
            ClientTransitionRequirement::AdapterDependent,
            t("transition.skipping_challenge"),
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            self.playback_state()
                .and_then(|playback| ensure_checkpoint(&playback, unit_path, step_id))?;
            let (challenge, status) = {
                let session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                let challenge = session
                    .challenge(unit_path, step_id)
                    .ok_or_else(|| t("error.not_a_challenge"))?;
                let status = session
                    .challenge_status(unit_path, step_id)
                    .unwrap_or(ChallengeStatus::NotStarted);
                (challenge, status)
            };
            if status == ChallengeStatus::Passed {
                return Err(t("error.challenge_already_passed"));
            }
            if status == ChallengeStatus::Skipped {
                return Ok(ChallengeCheckResult::new(ChallengeOutcome::Skipped));
            }
            if !challenge.allow_skip {
                return Err(t("error.challenge_skip_not_allowed"));
            }
            if !self.inner.transitions.transition_is_current(&ticket) {
                return Ok(ChallengeCheckResult::new(ChallengeOutcome::Cancelled));
            }
            self.record_challenge(unit_path, step_id, ChallengeOutcome::Skipped)?;
            Ok(ChallengeCheckResult::new(ChallengeOutcome::Skipped))
        }
        .await;
        self.finish_transition(ticket);
        result
    }

    pub(super) fn record_challenge(
        &self,
        unit_path: &str,
        step_id: &str,
        outcome: ChallengeOutcome,
    ) -> Result<(), String> {
        let state = {
            let mut session = self
                .inner
                .session
                .lock()
                .map_err(|_| t("error.course_session_unavailable"))?;
            match outcome {
                ChallengeOutcome::Passed => session.record_challenge_pass(unit_path, step_id),
                ChallengeOutcome::Failed => session.record_challenge_failure(unit_path, step_id),
                ChallengeOutcome::Skipped => session.record_challenge_skip(unit_path, step_id),
                ChallengeOutcome::WorkspaceDirty
                | ChallengeOutcome::Cancelled
                | ChallengeOutcome::Error => return Ok(()),
            }
            .map_err(|error| error.to_string())?;
            session.state()
        };
        let _ = self.inner.events.send(ClientEvent::Course(Arc::new(state)));
        Ok(())
    }
}
