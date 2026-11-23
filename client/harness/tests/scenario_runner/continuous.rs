use super::fixtures::*;
use super::*;

#[tokio::test]
async fn continuous_learning_demo_runs_without_external_toolchains() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("continuous-learning-demo"),
        unit_path: Some(format!("{}/{}", "node_one", "unit_one")),
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
        18
    );
    assert!(report.adapter_messages.iter().all(|message| {
        message.envelope.payload.get("command").is_none()
            && message.envelope.payload.get("session_key").is_none()
    }));
    for path in [
        "node_one/unit_one/unit-one.txt",
        "node_one/unit_two/unit-two.txt",
        "node_two/unit_three/unit-three.txt",
        "node_two/unit_four/unit-four.txt",
    ] {
        assert!(
            report.workspace_files.iter().any(|file| file == path),
            "missing {path}"
        );
    }
}

#[tokio::test]
async fn continuous_scenario_crosses_nodes_in_manifest_order() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("multi-node-demo"),
        unit_path: Some(format!("{}/{}", "ch_foundation", "main")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let transitions = report
        .events
        .iter()
        .filter(|event| event.kind == ScenarioEventKind::ContinuousUnitAdvanced)
        .map(|event| {
            (
                event.detail["from"]["unit_path"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                event.detail["to"]["unit_path"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(transitions.len(), 7);
    assert_eq!(
        transitions.first(),
        Some(&(
            "ch_foundation/main".to_string(),
            "ch_workspace_basics/main".to_string()
        ))
    );
    assert_eq!(
        transitions.last(),
        Some(&(
            "ch_automation/main".to_string(),
            "ch_deployment/main".to_string()
        ))
    );
    let course = report.course_state.active_course.unwrap();
    assert_eq!(
        course.current_unit_path, None,
        "traversal ended, so no resume cursor remains"
    );
    assert!(
        report.course_state.selected_unit.is_none(),
        "traversal ended without a selected Unit"
    );
    assert!(
        course
            .tree
            .iter()
            .flat_map(|node| node.children.iter())
            .filter_map(|node| node.unit.as_ref())
            .all(|unit| unit.status == cynapse_client_core::UnitStatus::Completed)
    );
}

#[tokio::test]
async fn continuous_scenario_uses_depth_first_order_across_a_mixed_course_tree() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("flexible-tree-demo"),
        unit_path: Some("intro".to_string()),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let transitions = report
        .events
        .iter()
        .filter(|event| event.kind == ScenarioEventKind::ContinuousUnitAdvanced)
        .map(|event| {
            (
                event.detail["from"]["unit_path"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                event.detail["to"]["unit_path"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        transitions,
        vec![
            ("intro".to_string(), "ch01_basics/what_is_node".to_string()),
            (
                "ch01_basics/what_is_node".to_string(),
                "ch01_basics/tree_and_recursion".to_string(),
            ),
            (
                "ch01_basics/tree_and_recursion".to_string(),
                "ch02_advanced/discriminated_union".to_string(),
            ),
            (
                "ch02_advanced/discriminated_union".to_string(),
                "ch02_advanced/ch02_02_abstraction/structural_role".to_string(),
            ),
            (
                "ch02_advanced/ch02_02_abstraction/structural_role".to_string(),
                "ch02_advanced/ch02_02_abstraction/business_semantics".to_string(),
            ),
            (
                "ch02_advanced/ch02_02_abstraction/business_semantics".to_string(),
                "appendix".to_string(),
            ),
        ]
    );
}

#[tokio::test]
async fn continuous_scenario_resumes_partial_units_and_resets_learned_ones() {
    let source = tempfile::tempdir().unwrap();
    continuous_resume_course(source.path());
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario_with_setup(
        ScenarioSpec {
            course_path: source.path().to_path_buf(),
            unit_path: Some(format!("{}/{}", "ch", "b")),
            mode: ScenarioMode::Continuous,
            adapter_script: AdapterScript::acknowledge_all(),
            adapter_script_path: None,
            artifact_root: artifacts.path().to_path_buf(),
            response_timeout: Duration::from_secs(3),
        },
        |session: &mut CourseSession| {
            session.start_unit(&format!("{}/{}", "ch", "a"), true)?;
            session.record_progress("ch/a", "a", true)?;
            session.start_unit(&format!("{}/{}", "ch", "b"), false)?;
            session.record_progress("ch/b", "b_first", true)?;
            session.start_unit(&format!("{}/{}", "ch", "c"), false)?;
            session.record_progress("ch/c", "c", true)?;
            session.start_unit(&format!("{}/{}", "ch", "b"), false)?;
            Ok(())
        },
    )
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert_eq!(
        report
            .events
            .iter()
            .filter(|event| event.kind == ScenarioEventKind::ActionReceived)
            .count(),
        3
    );
    let transitions = report
        .events
        .iter()
        .filter(|event| event.kind == ScenarioEventKind::ContinuousUnitAdvanced)
        .map(|event| {
            (
                event.detail["from"]["unit_path"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                event.detail["to"]["unit_path"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        transitions,
        vec![
            ("ch/b".to_string(), "ch/c".to_string()),
            ("ch/c".to_string(), "ch/d".to_string())
        ]
    );
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::ContinuousUnitReset
            && event.detail["unit_path"] == serde_json::json!("ch/c")
    }));
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path == "ch/b/b_second.txt")
    );
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path == "ch/d/d.txt")
    );
    assert!(
        !report
            .workspace_files
            .iter()
            .any(|path| path == "ch/b/b_first.txt")
    );
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path == "ch/c/c.txt")
    );
    let course = report.course_state.active_course.unwrap();
    assert!(course.has_progress());
    assert!(
        course
            .tree
            .iter()
            .flat_map(|node| node.children.iter())
            .filter_map(|node| node.unit.as_ref())
            .all(|unit| unit.status == cynapse_client_core::UnitStatus::Completed)
    );
}

#[tokio::test]
async fn any_unit_can_start_without_learning_dependencies() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("independent-units"),
        unit_path: Some(format!("{}/{}", "ch", "unit_d")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let active_course = report.course_state.active_course.unwrap();
    assert!(active_course.has_progress());
    let chapter = &active_course.tree[0];
    assert!(
        serde_json::to_value(chapter)
            .unwrap()
            .get("dependencies")
            .is_none()
    );
}

#[tokio::test]
async fn scenario_uses_unit_scoped_seeds_and_resolves_read_only_resources() {
    let artifacts = tempfile::tempdir().unwrap();
    let source = fixture("resources-and-seeds-demo");
    let report = run_scenario(ScenarioSpec {
        course_path: source.clone(),
        unit_path: Some(format!("{}/{}", "ch_foundation", "c_basics")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    for path in [
        "ch_foundation/c_basics/README.txt",
        "ch_foundation/c_basics/ancestor.txt",
        "ch_foundation/c_basics/main.c",
        "ch_foundation/c_basics/c_report.txt",
    ] {
        assert!(
            report.workspace_files.iter().any(|file| file == path),
            "missing {path}: {:?}",
            report.workspace_files
        );
    }
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path.starts_with("resources/include/")),
        "workspace resources: {:?}",
        report.workspace_files
    );
    assert!(
        report
            .workspace_contents
            .keys()
            .all(|path| !path.starts_with("resources/")),
        "resource files must not be captured as report contents"
    );
    let commands = report
        .adapter_messages
        .iter()
        .filter(|message| message.direction == AdapterDirection::Received)
        .filter_map(|message| message.envelope.payload["command"].as_str())
        .map(|command| command.replace('\\', "/"))
        .collect::<Vec<_>>();
    assert!(
        commands
            .iter()
            .any(|command| command.contains("resources/include"))
    );
    assert!(
        commands
            .iter()
            .any(|command| command.contains("resources/ch_foundation/c_basics/data.txt"))
    );
    assert!(
        commands
            .iter()
            .all(|command| !command.contains("{{resource:"))
    );
    let session_keys = report
        .adapter_messages
        .iter()
        .filter(|message| message.direction == AdapterDirection::Received)
        .filter_map(|message| message.envelope.payload["session_key"].as_str())
        .collect::<Vec<_>>();
    assert!(session_keys.contains(&"ch_foundation/c_basics::main"));
    assert!(session_keys.contains(&"ch_foundation/c_basics::aux"));
}

#[tokio::test]
async fn scenario_preserves_multiple_c_mains_in_one_unit() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("resources-and-seeds-demo"),
        unit_path: Some(format!("{}/{}", "ch_foundation", "c_multiple_programs")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    for path in [
        "ch_foundation/c_multiple_programs/program_a/main.c",
        "ch_foundation/c_multiple_programs/program_b/main.c",
        "ch_foundation/c_multiple_programs/shared/common.h",
    ] {
        assert!(
            report.workspace_files.iter().any(|file| file == path),
            "missing {path}: {:?}",
            report.workspace_files
        );
    }
    let commands = report
        .adapter_messages
        .iter()
        .filter(|message| message.direction == AdapterDirection::Received)
        .filter(|message| message.envelope.payload["command"].is_string())
        .map(|message| message.envelope.payload["cwd"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(commands.contains(&"ch_foundation/c_multiple_programs/program_a"));
    assert!(commands.contains(&"ch_foundation/c_multiple_programs/program_b"));
}

#[tokio::test]
async fn scenario_stop_after_step_preserves_progress_and_closes_terminals_only() {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir_all(source.path().join("workspace")).unwrap();
    fs::create_dir_all(source.path().join("steps")).unwrap();
    fs::write(
        source.path().join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "stop_demo",
            "title": "Stop Demo",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": [
                        {"step_file": "steps/first.json"},
                        {"step_file": "steps/second.json"}
                    ]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        source.path().join("steps/first.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "first",
            "title": "First",
            "actions": [
                {"type": "CreateFile", "path": "first.txt"},
                {
                    "type": "TerminalEnsure",
                    "session_key": "main",
                    "cwd": "."
                },
                {
                    "type": "TerminalCommand",
                    "session_key": "main",
                    "cwd": ".",
                    "command": "echo first"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        source.path().join("steps/second.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "second",
            "title": "Second",
            "actions": [{
                "type": "CreateFile",
                "path": "second.txt"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some(format!("{}/{}", "ch", "unit")),
        mode: ScenarioMode::StopAfterStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Stopped);
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path == "ch/unit/first.txt")
    );
    assert!(
        !report
            .workspace_files
            .iter()
            .any(|path| path == "ch/unit/second.txt")
    );
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::ResumeCaptured
            && event.detail["step_id"] == serde_json::json!("second")
    }));
    assert!(report.adapter_messages.iter().any(|message| {
        message.direction == AdapterDirection::Received
            && message.envelope.kind == cynapse_client_core::MessageType::WorkspaceReset
            && message.envelope.payload["paths"] == serde_json::json!(["ch/unit"])
            && message.envelope.payload["close_tabs"] == false
    }));
    let course = report.course_state.active_course.unwrap();
    assert_eq!(course.current_unit_path, None);
    assert_eq!(
        unit_by_path(&course.tree, "ch/unit").status,
        cynapse_client_core::UnitStatus::InProgress
    );
}
