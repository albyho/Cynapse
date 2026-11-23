use super::*;
use cynapse_client_i18n::{t, t_with};

impl PlaybackCommandSender {
    pub async fn check_environment(
        &self,
        requirements: &[EnvironmentRequirement],
        unit_path: &str,
        session_key: &str,
        cwd: &str,
    ) -> EnvironmentCheckReport {
        let ensure = self
            .execute_action(
                Action::TerminalEnsure(TerminalEnsureAction {
                    session_key: session_key.to_string(),
                    cwd: cwd.to_string(),
                }),
                unit_path,
            )
            .ok()
            .map(|receiver| tokio::time::timeout(Duration::from_secs(10), receiver));
        let ensure_ack = match ensure {
            Some(timeout) => timeout.await,
            None => {
                return environment_failure(
                    "terminal",
                    t("error.terminal_session_unavailable"),
                    None,
                );
            }
        };
        let ensure_ack = match ensure_ack {
            Ok(Ok(Ok(ack))) => ack,
            Ok(Ok(Err(message))) => {
                return self
                    .finish_environment_check(environment_failure("terminal", message, None))
                    .await;
            }
            Ok(Err(_)) => {
                return self
                    .finish_environment_check(environment_failure(
                        "terminal",
                        "Terminal Session acknowledgement channel closed".to_string(),
                        None,
                    ))
                    .await;
            }
            Err(_) => {
                return self
                    .finish_environment_check(environment_failure(
                        "terminal",
                        "Timed out waiting for the Terminal Session".to_string(),
                        None,
                    ))
                    .await;
            }
        };
        if ensure_ack.status != AckStatus::Done {
            return self
                .finish_environment_check(environment_failure(
                    "terminal",
                    ensure_ack
                        .message
                        .unwrap_or_else(|| t("error.terminal_session_create_failed")),
                    ensure_ack.output,
                ))
                .await;
        }

        let mut checks = Vec::with_capacity(requirements.len());
        for requirement in requirements {
            let request = self
                .execute_action(
                    Action::TerminalCommand(TerminalCommandAction {
                        session_key: session_key.to_string(),
                        cwd: cwd.to_string(),
                        command: requirement.command.clone(),
                        accepted_exit_codes: vec![0],
                        expected_output: Vec::new(),
                        timeout_ms: 30_000,
                        completion_marker: None,
                        new_terminal: false,
                    }),
                    unit_path,
                )
                .ok();
            let ack = match request {
                Some(receiver) => {
                    match tokio::time::timeout(Duration::from_secs(35), receiver).await {
                        Ok(Ok(Ok(ack))) => ack,
                        Ok(Ok(Err(message))) => {
                            checks.push(failed_environment_check(requirement, message, None));
                            continue;
                        }
                        Ok(Err(_)) => {
                            checks.push(failed_environment_check(
                                requirement,
                                "Environment check acknowledgement channel closed".to_string(),
                                None,
                            ));
                            continue;
                        }
                        Err(_) => {
                            checks.push(failed_environment_check(
                                requirement,
                                "Timed out waiting for the environment check".to_string(),
                                None,
                            ));
                            continue;
                        }
                    }
                }
                None => {
                    checks.push(failed_environment_check(
                        requirement,
                        t("error.playback_server_unavailable"),
                        None,
                    ));
                    continue;
                }
            };

            if ack.status != AckStatus::Done {
                checks.push(failed_environment_check(
                    requirement,
                    ack.message.unwrap_or_else(|| {
                        t_with(
                            "error.environment_command_failed",
                            &[("command", &requirement.command)],
                        )
                    }),
                    ack.output,
                ));
                continue;
            }

            if ack.exit_code != Some(0) {
                checks.push(failed_environment_check(
                    requirement,
                    format!(
                        "{} did not report an accepted exit code",
                        requirement.command
                    ),
                    ack.output,
                ));
                continue;
            }
            let output = ack.output.unwrap_or_default();
            if requirement
                .minimum_version
                .as_deref()
                .is_some_and(|minimum| !output_satisfies_minimum_version(&output, minimum))
            {
                let minimum = requirement.minimum_version.as_deref().unwrap_or_default();
                checks.push(EnvironmentCheckItem {
                    id: requirement.id.clone(),
                    ok: false,
                    message: format!(
                        "{} requires version {minimum} or newer. {}{}",
                        requirement.id,
                        requirement.install_hint,
                        if requirement.install_hint.is_empty() {
                            ""
                        } else {
                            " "
                        }
                    )
                    .trim_end()
                    .to_string(),
                    output: Some(output),
                });
                continue;
            }
            checks.push(EnvironmentCheckItem {
                id: requirement.id.clone(),
                ok: true,
                message: "environment requirement satisfied".to_string(),
                output: Some(output),
            });
        }

        let report = EnvironmentCheckReport {
            ok: checks.iter().all(|check| check.ok),
            checks,
        };
        self.finish_environment_check(report).await
    }

    async fn finish_environment_check(
        &self,
        mut report: EnvironmentCheckReport,
    ) -> EnvironmentCheckReport {
        // Failed checks keep the terminal so the learner can inspect and
        // repair the environment. Only a successful check closes it.
        if report.ok
            && let Some(message) = self.cleanup_environment_terminal().await.err()
        {
            report.ok = false;
            report.checks.push(EnvironmentCheckItem {
                id: "terminal_cleanup".to_string(),
                ok: false,
                message,
                output: None,
            });
        }
        report
    }

    async fn cleanup_environment_terminal(&self) -> Result<(), String> {
        // Only this check's own session: an empty path scope would also close
        // terminals the previous Unit left open in the same Course Workspace.
        let completion = self
            .terminal_cleanup_sessions(vec![ENVIRONMENT_CHECK_SESSION_KEY.to_string()])
            .map_err(|_| t("error.environment_cleanup_unavailable"))?;
        match tokio::time::timeout(Duration::from_secs(10), completion).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(message))) => Err(format!(
                "Environment check terminal could not be closed: {message}"
            )),
            Ok(Err(_)) => {
                Err("Environment check terminal cleanup acknowledgement was lost".to_string())
            }
            Err(_) => Err("Timed out closing the environment check terminal".to_string()),
        }
    }
}

fn environment_failure(
    id: &str,
    message: String,
    output: Option<String>,
) -> EnvironmentCheckReport {
    EnvironmentCheckReport {
        ok: false,
        checks: vec![EnvironmentCheckItem {
            id: id.to_string(),
            ok: false,
            message,
            output,
        }],
    }
}

fn failed_environment_check(
    requirement: &EnvironmentRequirement,
    message: String,
    output: Option<String>,
) -> EnvironmentCheckItem {
    EnvironmentCheckItem {
        id: requirement.id.clone(),
        ok: false,
        message: if requirement.install_hint.trim().is_empty() {
            message
        } else {
            format!("{message} {}", requirement.install_hint.trim())
        },
        output,
    }
}
