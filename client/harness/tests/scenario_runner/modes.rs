use super::*;

#[tokio::test]
async fn setup_errors_retain_artifacts_and_print_agent_recovery_instructions() {
    let source = tempfile::tempdir().unwrap();
    let artifacts = tempfile::tempdir().unwrap();
    let error = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some("missing".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap_err();

    let ScenarioError::Execution {
        artifact_path,
        repro_command,
        cleanup_command,
        ..
    } = &error
    else {
        panic!("expected artifact-aware scenario error");
    };
    assert!(PathBuf::from(artifact_path).join("failure.json").is_file());
    assert!(repro_command.contains("--unit \"missing\""));
    assert!(cleanup_command.contains(artifact_path));
}

#[tokio::test]
async fn by_step_scenario_executes_actions_and_reports_observable_state() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-step-demo"),
        unit_path: Some(format!("{}/{}", "ch_multi_step", "main")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert!(!report.correlation_id.is_empty());
    assert_eq!(report.adapter_messages.len(), 10);
    assert_eq!(
        report
            .events
            .iter()
            .filter(|event| event.kind == ScenarioEventKind::ActionReceived)
            .count(),
        5
    );
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path == "ch_multi_step/main/multi_step_demo.txt")
    );
    assert!(!report.artifacts_retained);
    assert!(!PathBuf::from(&report.artifact_path).exists());
    assert!(
        report
            .events
            .iter()
            .all(|event| { event.correlation_id == report.correlation_id })
    );
}

#[tokio::test]
async fn presentation_scenario_uses_a_no_op_action_without_workspace_changes() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("presentation-demo"),
        unit_path: Some(format!("{}/{}", "ch_presentation", "pure_presentation")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let actions = report
        .adapter_messages
        .iter()
        .filter(|message| message.direction == AdapterDirection::Received)
        .map(|message| &message.envelope.payload)
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], &serde_json::json!({"type": "Present"}));
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::NarrationCompleted
            && event.detail["clip"]["phase"] == serde_json::json!("before")
            && event.detail["clip"]["action_index"] == serde_json::json!(0)
            && event.detail["clip"]["clip_index"] == serde_json::json!(0)
    }));
    assert!(report.adapter_messages.iter().all(|message| {
        message.envelope.payload.get("path").is_none()
            && message.envelope.payload.get("command").is_none()
    }));
}

#[tokio::test]
async fn scenario_prepares_the_declared_workspace_not_the_fallback() {
    let source = tempfile::tempdir().unwrap();
    let course = source.path().join("course");
    fs::create_dir_all(course.join("steps")).unwrap();
    fs::write(
        course.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "custom_workspace",
            "title": "Custom Workspace",
            "adapters": ["vscode"],
            "workspace": "workspace/custom",
            "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": [{"step_file": "steps/step.json"}]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        course.join("steps/step.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: course,
        unit_path: Some(format!("{}/{}", "ch", "unit")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    let prepared_workspace = report
        .events
        .iter()
        .find(|event| event.kind == ScenarioEventKind::WorkspacePrepared)
        .and_then(|event| event.detail["path"].as_str());
    // Compare the stable suffix; Windows may spell the temp root as RUNNER~1
    // or runneradmin.
    let artifact_name = Path::new(&report.artifact_path).file_name().unwrap();
    let expected_workspace = Path::new(artifact_name)
        .join("course")
        .join("workspace")
        .join("custom");
    assert!(Path::new(prepared_workspace.unwrap()).ends_with(expected_workspace));
}

#[tokio::test]
async fn continuous_scenario_crosses_units_and_completes_the_final_unit() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-unit-demo"),
        unit_path: Some(format!("{}/{}", "ch_units", "first_unit")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert_eq!(
        report
            .events
            .iter()
            .filter(|event| event.kind == ScenarioEventKind::ActionReceived)
            .count(),
        6
    );
    let course = report.course_state.active_course.unwrap();
    assert_eq!(course.current_unit_path, None);
    assert!(
        course
            .tree
            .iter()
            .flat_map(|node| node.children.iter())
            .filter_map(|node| node.unit.as_ref())
            .all(|unit| unit.status == cynapse_client_core::UnitStatus::Completed)
    );
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path == "ch_units/first_unit/multi_unit_demo.txt")
    );
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path == "ch_units/second_unit/multi_unit_demo.txt")
    );
    assert!(report.adapter_messages.iter().all(|message| {
        message.envelope.kind != cynapse_client_core::MessageType::WorkspaceReset
    }));
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::LearningState
            && event.detail["mode"] == serde_json::json!("continuous")
    }));
}

#[tokio::test]
async fn continuous_scenario_advances_steps_without_manual_next() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-step-demo"),
        unit_path: Some(format!("{}/{}", "ch_multi_step", "main")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert_eq!(
        report
            .events
            .iter()
            .filter(|event| event.kind == ScenarioEventKind::ActionReceived)
            .count(),
        5
    );
    let course = report
        .course_state
        .active_course
        .expect("the course stays open");
    assert_eq!(
        course.current_unit_path, None,
        "traversal ended, so no resume cursor remains"
    );
    let unit = unit_by_path(&course.tree, "ch_multi_step/main");
    assert!(unit.completed);
    assert_eq!(unit.status, cynapse_client_core::UnitStatus::Completed);
    assert!(report.course_state.selected_unit.is_none());
}

#[tokio::test]
async fn continuous_scenario_stops_at_a_human_step_and_retains_the_unit() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("manual-boundary-demo"),
        unit_path: Some(format!("{}/{}", "ch", "unit")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::WaitingForHuman);
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::HumanStepReached
            && event.detail["step_id"] == serde_json::json!("manual")
    }));
    let course = report.course_state.active_course.unwrap();
    assert_eq!(course.current_unit_path.as_deref(), Some("ch/unit"));
    assert!(course.has_progress());
}

#[tokio::test]
async fn a_human_final_step_completes_the_final_unit_without_waiting_for_human() {
    let source = tempfile::tempdir().unwrap();
    fixtures::human_final_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some("ch/unit".to_string()),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let course = report.course_state.active_course.unwrap();
    assert!(course.has_progress());
    assert!(unit_by_path(&course.tree, "ch/unit").completed);
}

#[tokio::test]
async fn continuous_scenario_pauses_at_a_human_unit_boundary() {
    let source = tempfile::tempdir().unwrap();
    fixtures::human_final_before_next_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some("ch/unit".to_string()),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::WaitingForHuman);
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::HumanStepReached
            && event.detail["step_id"] == serde_json::json!("final")
    }));
    let course = report.course_state.active_course.unwrap();
    assert!(course.has_progress());
    assert!(unit_by_path(&course.tree, "ch/unit").completed);
    assert!(!unit_by_path(&course.tree, "ch/next").completed);
}
