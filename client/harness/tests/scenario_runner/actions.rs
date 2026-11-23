use super::fixtures::*;
use super::*;

#[tokio::test]
async fn scenario_applies_open_and_insert_actions_to_an_existing_file() {
    let source = tempfile::tempdir().unwrap();
    insert_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some("editing/insert".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let inserted = report
        .workspace_contents
        .iter()
        .find(|(path, _)| path.as_str() == "editing/insert/main.rs")
        .map(|(_, content)| content)
        .expect("the Unit workspace should be captured");
    assert_eq!(inserted, "fn main() {\n    println!(\"ok\");\n}\n");
    assert!(report.adapter_messages.iter().any(|message| {
        message.direction == AdapterDirection::Received
            && message.envelope.payload["type"] == serde_json::json!("OpenFile")
    }));
    assert_eq!(
        report
            .adapter_messages
            .iter()
            .filter(|message| {
                message.direction == AdapterDirection::Received
                    && message.envelope.payload["type"] == serde_json::json!("InsertText")
            })
            .count(),
        2
    );
}

#[tokio::test]
async fn scenario_applies_delete_and_replace_actions_to_an_open_file() {
    let source = tempfile::tempdir().unwrap();
    edit_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some("editing/edit".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let edited = report
        .workspace_contents
        .iter()
        .find(|(path, _)| path.as_str() == "editing/edit/main.rs")
        .map(|(_, content)| content)
        .expect("the Unit workspace should be captured");
    assert_eq!(edited, "fn main() {\n    println!(\"new\");\n}\n");
    assert_eq!(
        report
            .adapter_messages
            .iter()
            .filter(|message| {
                message.direction == AdapterDirection::Received
                    && message.envelope.payload["type"] == serde_json::json!("DeleteText")
            })
            .count(),
        1
    );
    assert_eq!(
        report
            .adapter_messages
            .iter()
            .filter(|message| {
                message.direction == AdapterDirection::Received
                    && message.envelope.payload["type"] == serde_json::json!("ReplaceText")
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn invalid_unit_action_path_surfaces_instead_of_empty_playback_steps() {
    let source = tempfile::tempdir().unwrap();
    fixtures::bad_runtime_action_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();

    let error = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some("ch/unit".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::default(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("resource alias"), "{message}");
}

#[tokio::test]
async fn scenario_completes_create_open_insert_delete_replace_compile_and_run() {
    let source = tempfile::tempdir().unwrap();
    complete_edit_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some("editing/complete".to_string()),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert!(
        report
            .workspace_files
            .contains(&"editing/complete/main.rs".to_string())
    );
    let action_types = report
        .adapter_messages
        .iter()
        .filter(|message| message.direction == AdapterDirection::Received)
        .filter_map(|message| message.envelope.payload["type"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        action_types,
        vec![
            "CreateFile",
            "OpenFile",
            "InsertText",
            "TerminalEnsure",
            "TerminalCommand",
            "OpenFile",
            "DeleteText",
            "ReplaceText",
            "TerminalCommand",
            "TerminalCommand",
        ]
    );
    let commands = report
        .adapter_messages
        .iter()
        .filter(|message| message.direction == AdapterDirection::Received)
        .filter_map(|message| message.envelope.payload["command"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        commands,
        vec!["rustc main.rs", "rustc main.rs", ".\\main.exe"]
    );
}

#[tokio::test]
async fn timed_out_adapter_reply_keeps_artifacts_and_reports_reproduction_command() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-step-demo"),
        unit_path: Some(format!("{}/{}", "ch_multi_step", "main")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: Vec::new(),
            replies: vec![AdapterReply::Timeout],
            default_reply: AdapterReply::Timeout,
            narration: Default::default(),
        },
        adapter_script_path: Some(PathBuf::from("scenario.json")),
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_millis(50),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::TimedOut);
    assert!(report.artifacts_retained);
    assert!(
        report
            .repro_command
            .contains("--unit \"ch_multi_step/main\"")
    );
    assert!(
        report
            .repro_command
            .contains("--adapter-script \"scenario.json\"")
    );
    assert!(report.cleanup_command.contains(&report.artifact_path));
    assert!(
        fs::read_dir(&report.artifact_path)
            .unwrap()
            .any(|entry| entry.unwrap().file_name() == "report.json")
    );
    assert!(
        fs::read_dir(&report.artifact_path)
            .unwrap()
            .any(|entry| entry.unwrap().file_name() == "events.jsonl")
    );
    assert!(PathBuf::from(&report.logs_path).is_file());
}

#[tokio::test]
async fn failed_action_is_reported_as_a_playback_error() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-step-demo"),
        unit_path: Some(format!("{}/{}", "ch_multi_step", "main")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            check_replies: Vec::new(),
            replies: vec![AdapterReply::Acknowledge {
                payload: AckPayload {
                    status: AckStatus::Error,
                    code: Some("write_failed".to_string()),
                    message: Some("scripted failure".to_string()),
                    exit_code: None,
                    output: None,
                    output_truncated: None,
                    completion_marker_found: None,
                    timed_out: None,
                },
            }],
            default_reply: AdapterReply::default(),
            narration: Default::default(),
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::PlaybackError);
    assert!(report.artifacts_retained);
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::PlaybackError
            && event.detail["code"] == serde_json::json!("write_failed")
    }));
}

#[tokio::test]
async fn failed_terminal_command_is_reported_without_completing_the_unit() {
    let source = tempfile::tempdir().unwrap();
    terminal_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some(format!("{}/{}", "terminal", "run")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript {
            check_replies: Vec::new(),
            replies: vec![
                AdapterReply::default(),
                AdapterReply::Acknowledge {
                    payload: AckPayload {
                        status: AckStatus::Error,
                        code: Some("terminal_exit_code_rejected".to_string()),
                        message: Some("command failed".to_string()),
                        exit_code: Some(1),
                        output: Some("error".to_string()),
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

    assert_eq!(report.status, ScenarioStatus::PlaybackError);
    let course = report.course_state.active_course.unwrap();
    assert_eq!(course.current_unit_path.as_deref(), Some("terminal/run"));
}

#[tokio::test]
async fn terminal_timeout_is_reported_without_completing_the_unit() {
    let source = tempfile::tempdir().unwrap();
    terminal_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some(format!("{}/{}", "terminal", "run")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript {
            check_replies: Vec::new(),
            replies: vec![
                AdapterReply::default(),
                AdapterReply::Acknowledge {
                    payload: AckPayload {
                        status: AckStatus::Cancelled,
                        code: Some("terminal_timeout".to_string()),
                        message: Some("command timed out".to_string()),
                        exit_code: None,
                        output: Some("partial".to_string()),
                        output_truncated: Some(false),
                        completion_marker_found: None,
                        timed_out: Some(true),
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

    assert_eq!(report.status, ScenarioStatus::PlaybackError);
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::PlaybackError
            && event.detail["code"] == serde_json::json!("terminal_timeout")
    }));
    let course = report.course_state.active_course.unwrap();
    assert_eq!(course.current_unit_path.as_deref(), Some("terminal/run"));
}
