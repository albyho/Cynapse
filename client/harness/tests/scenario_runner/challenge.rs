use super::*;

#[tokio::test]
async fn challenge_fails_then_passes_after_the_fixture_solution_is_applied() {
    let artifacts = tempfile::tempdir().unwrap();
    let failed_check = AdapterReply::Acknowledge {
        payload: AckPayload {
            status: AckStatus::Error,
            code: Some("terminal_exit_code_rejected".to_string()),
            message: Some("Command exited with code 1".to_string()),
            exit_code: Some(1),
            output: Some("not ok".to_string()),
            output_truncated: None,
            completion_marker_found: None,
            timed_out: None,
        },
    };
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("challenge-fix-test"),
        unit_path: Some("ch/main".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: vec![failed_check, AdapterReply::default()],
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let checks = report
        .adapter_messages
        .iter()
        .filter(|message| {
            message.direction == AdapterDirection::Received
                && message.envelope.kind == MessageType::Check
        })
        .count();
    assert_eq!(checks, 2);
    for message in report.adapter_messages.iter().filter(|message| {
        message.direction == AdapterDirection::Received
            && message.envelope.kind == MessageType::Check
    }) {
        assert_eq!(
            message.envelope.payload["session_key"], "ch/main::main",
            "the Check must reuse the Unit-scoped Terminal Session"
        );
    }

    let fixed = report
        .workspace_contents
        .iter()
        .find(|(path, _)| *path == "ch/main/src/add.js")
        .map(|(_, content)| content)
        .expect("the fixture solution should be copied into the Unit Workspace");
    assert!(fixed.contains("a + b"), "{fixed}");

    let course = report
        .course_state
        .active_course
        .as_ref()
        .expect("the scenario should keep the active course");
    let unit = unit_by_path(&course.tree, "ch/main");
    let step = unit
        .steps
        .iter()
        .find(|step| step.step_id == "fix_add")
        .expect("the challenge step should be visible");
    assert_eq!(step.challenge_status, ChallengeStatus::Passed);
    assert_eq!(step.challenge_attempts, 2);
}

fn failed_check(code: &str, message: &str) -> AdapterReply {
    AdapterReply::Acknowledge {
        payload: AckPayload {
            status: AckStatus::Error,
            code: Some(code.to_string()),
            message: Some(message.to_string()),
            exit_code: None,
            output: None,
            output_truncated: None,
            completion_marker_found: None,
            timed_out: None,
        },
    }
}

fn check_codes(report: &cynapse_client_harness::ScenarioReport) -> Vec<String> {
    report
        .events
        .iter()
        .filter(|event| {
            event.kind == ScenarioEventKind::AdapterMessageSent
                && event.detail["kind"] == serde_json::json!("check")
        })
        .filter_map(|event| event.detail["code"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
async fn challenge_timeout_fails_then_passes_on_retry() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("challenge-fix-test"),
        unit_path: Some("ch/main".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: vec![
                failed_check("terminal_timeout", "Command timed out"),
                AdapterReply::default(),
            ],
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert_eq!(check_codes(&report), ["terminal_timeout"]);
    let course = report.course_state.active_course.as_ref().unwrap();
    let step = unit_by_path(&course.tree, "ch/main")
        .steps
        .iter()
        .find(|step| step.step_id == "fix_add")
        .unwrap();
    assert_eq!(step.challenge_status, ChallengeStatus::Passed);
    assert_eq!(step.challenge_attempts, 2);
}

#[tokio::test]
async fn challenge_output_mismatch_fails_then_passes_on_retry() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("challenge-fix-test"),
        unit_path: Some("ch/main".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: vec![
                failed_check("terminal_output_mismatch", "Expected output missing"),
                AdapterReply::default(),
            ],
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert_eq!(check_codes(&report), ["terminal_output_mismatch"]);
    let course = report.course_state.active_course.as_ref().unwrap();
    let step = unit_by_path(&course.tree, "ch/main")
        .steps
        .iter()
        .find(|step| step.step_id == "fix_add")
        .unwrap();
    assert_eq!(step.challenge_status, ChallengeStatus::Passed);
    assert_eq!(step.challenge_attempts, 2);
}

#[tokio::test]
async fn challenge_skip_after_failure_marks_the_step_skipped() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario_with_options(
        ScenarioSpec {
            course_path: fixture("challenge-fix-test"),
            unit_path: Some("ch/main".to_string()),
            mode: ScenarioMode::ByStep,
            adapter_script: AdapterScript {
                check_replies: vec![failed_check(
                    "terminal_exit_code_rejected",
                    "Command exited with code 1",
                )],
                ..AdapterScript::default()
            },
            adapter_script_path: None,
            artifact_root: artifacts.path().to_path_buf(),
            response_timeout: Duration::from_secs(3),
        },
        ScenarioOptions {
            skip_challenge: true,
            ..ScenarioOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let course = report.course_state.active_course.as_ref().unwrap();
    let unit = unit_by_path(&course.tree, "ch/main");
    let step = unit
        .steps
        .iter()
        .find(|step| step.step_id == "fix_add")
        .unwrap();
    assert_eq!(step.challenge_status, ChallengeStatus::Skipped);
    assert_eq!(step.challenge_attempts, 1);
    assert_eq!(unit.skipped_challenges, 1);
}
