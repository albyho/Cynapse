use super::fixtures::*;
use super::*;

#[tokio::test]
async fn scripted_disconnect_is_reported_and_retained() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-step-demo"),
        unit_path: Some(format!("{}/{}", "ch_multi_step", "main")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: Vec::new(),
            replies: vec![AdapterReply::Disconnect],
            default_reply: AdapterReply::default(),
            narration: Default::default(),
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::AdapterDisconnected);
    assert!(report.artifacts_retained);
}

#[tokio::test]
async fn scripted_cancel_records_the_cancel_round_trip() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-step-demo"),
        unit_path: Some(format!("{}/{}", "ch_multi_step", "main")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: Vec::new(),
            replies: vec![AdapterReply::Cancel],
            default_reply: AdapterReply::default(),
            narration: Default::default(),
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Cancelled);
    assert!(report.adapter_messages.iter().any(|message| {
        message.direction == AdapterDirection::Received
            && message.envelope.kind == cynapse_client_core::MessageType::Cancel
    }));
    assert!(report.adapter_messages.iter().any(|message| {
        message.direction == AdapterDirection::Sent
            && message.envelope.payload["status"] == serde_json::json!("cancelled")
    }));
}

#[tokio::test]
async fn scripted_terminal_output_is_returned_without_a_real_terminal() {
    let source = tempfile::tempdir().unwrap();
    terminal_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some(format!("{}/{}", "terminal", "run")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: Vec::new(),
            replies: vec![
                AdapterReply::default(),
                AdapterReply::Acknowledge {
                    payload: AckPayload {
                        status: AckStatus::Done,
                        code: None,
                        message: None,
                        exit_code: Some(0),
                        output: Some("harness-ok\n".to_string()),
                        output_truncated: Some(false),
                        completion_marker_found: None,
                        timed_out: None,
                    },
                },
            ],
            default_reply: AdapterReply::default(),
            narration: Default::default(),
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let course = serde_json::to_value(&report.course_state).unwrap();
    assert!(course["active_course"]["tree"].is_array());
    assert!(course["active_course"].get("modules").is_none());
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::ActionReceived
            && event.detail["payload"]["command"] == serde_json::json!("example --version")
    }));
    assert!(report.adapter_messages.iter().any(|message| {
        message.direction == AdapterDirection::Sent
            && message.envelope.payload["output"] == serde_json::json!("harness-ok\n")
    }));
}
