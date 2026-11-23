use cynapse_client_core::CourseSession;
use cynapse_client_harness::{
    AdapterScript, ScenarioMode, ScenarioReport, ScenarioSpec, ScenarioStatus, run_scenario,
    run_scenario_with_setup,
};
use std::fs;
use std::path::Path;
use std::time::Duration;

mod common;

use common::unit_by_path;

/// A course whose editing Steps cover InsertText, DeleteText and
/// ReplaceText, with anchors that stay stable across each Action's own
/// partial states so the scenario harness can mirror the Adapter's Recovery
/// Self-heal (ADR-0039, issue #74).
fn recovery_self_heal_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "recovery_self_heal",
            "title": "Recovery Self-heal",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Chapter",
                "children": [{
                    "id": "recover",
                    "title": "Recover",
                    "steps": [
                        { "step_file": "steps/write.json" },
                        { "step_file": "steps/extend.json" },
                        { "step_file": "steps/cleanup.json" },
                        { "step_file": "steps/polish.json" }
                    ]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let steps = [
        (
            "write",
            serde_json::json!([
                { "type": "CreateFile", "path": "main.rs" },
                { "type": "OpenFile", "path": "main.rs" },
                {
                    "type": "InsertText",
                    "path": "main.rs",
                    "target": {"line": 1},
                    "text": "fn main() {\n    // temporary\n    println!(\"old\");\n}\n"
                }
            ]),
        ),
        (
            "extend",
            serde_json::json!([
                { "type": "OpenFile", "path": "main.rs" },
                {
                    "type": "InsertText",
                    "path": "main.rs",
                    "target": {"eof": true},
                    "text": "// tail\n",
                    "speed_ms": 1
                }
            ]),
        ),
        (
            "cleanup",
            serde_json::json!([
                { "type": "OpenFile", "path": "main.rs" },
                {
                    "type": "DeleteText",
                    "path": "main.rs",
                    "target": {
                        "start": {"at": {"text": "fn main() {\n"}, "placement": "after"},
                        "end": {"at": {"text": "    println!(\"old\");"}, "placement": "before"},
                        "expected_text": "    // temporary\n"
                    },
                    "direction": "delete"
                }
            ]),
        ),
        (
            "polish",
            serde_json::json!([
                { "type": "OpenFile", "path": "main.rs" },
                {
                    "type": "ReplaceText",
                    "path": "main.rs",
                    "target": {
                        "start": {"at": {"text": "fn main() {\n"}, "placement": "after"},
                        "end": {"at": {"text": ";\n}"}, "placement": "before"},
                        "expected_text": "    println!(\"old\")"
                    },
                    "text": "    println!(\"new\")",
                    "edit_mode": "delete_then_type",
                    "speed_ms": 1
                }
            ]),
        ),
    ];
    for (step_id, actions) in steps {
        fs::write(
            root.join("steps").join(format!("{step_id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "step_id": step_id,
                "title": step_id,
                "actions": actions
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

async fn run_recovery_scenario(
    source: &Path,
    mode: ScenarioMode,
    artifact_root: &Path,
    completed_steps: &[&str],
    crashed_step: &str,
    crashed_file: &str,
) -> ScenarioReport {
    let unit_path = format!("{}/{}", "ch", "recover");
    let crashed_step_id = crashed_step.to_string();
    run_scenario_with_setup(
        ScenarioSpec {
            course_path: source.to_path_buf(),
            unit_path: Some(unit_path.clone()),
            mode,
            adapter_script: AdapterScript::acknowledge_all(),
            adapter_script_path: None,
            artifact_root: artifact_root.to_path_buf(),
            response_timeout: Duration::from_secs(3),
        },
        move |session: &mut CourseSession| {
            session.start_unit(&unit_path, true)?;
            for step_id in completed_steps {
                session.record_progress(&unit_path, step_id, true)?;
            }
            session.record_progress(&unit_path, &crashed_step_id, false)?;
            let workspace = session
                .active_workspace()
                .expect("the unit workspace is prepared");
            fs::write(workspace.join(&unit_path).join("main.rs"), crashed_file)?;
            Ok(())
        },
    )
    .await
    .unwrap()
}

async fn uninterrupted_recovery_reference(
    source: &Path,
    mode: ScenarioMode,
    artifact_root: &Path,
) -> ScenarioReport {
    run_scenario(ScenarioSpec {
        course_path: source.to_path_buf(),
        unit_path: Some(format!("{}/{}", "ch", "recover")),
        mode,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifact_root.to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap()
}

fn assert_recovery_matches_reference(report: &ScenarioReport, reference: &ScenarioReport) {
    assert_eq!(report.status, ScenarioStatus::Completed);
    assert_eq!(reference.status, ScenarioStatus::Completed);
    assert_eq!(report.workspace_files, reference.workspace_files);
    assert_eq!(
        report.workspace_contents, reference.workspace_contents,
        "the crashed run must reach the uninterrupted terminal workspace"
    );
    let course = report.course_state.active_course.clone().unwrap();
    assert_eq!(
        unit_by_path(&course.tree, "ch/recover").status,
        cynapse_client_core::UnitStatus::Completed
    );
    let reference_course = reference.course_state.active_course.clone().unwrap();
    assert_eq!(
        unit_by_path(&reference_course.tree, "ch/recover").status,
        cynapse_client_core::UnitStatus::Completed
    );
}

#[tokio::test]
async fn crashed_typewriter_insert_recovers_to_the_uninterrupted_workspace() {
    let source = tempfile::tempdir().unwrap();
    recovery_self_heal_course(source.path());
    let reference_artifacts = tempfile::tempdir().unwrap();
    let reference = uninterrupted_recovery_reference(
        source.path(),
        ScenarioMode::ByStep,
        reference_artifacts.path(),
    )
    .await;
    let crashed_artifacts = tempfile::tempdir().unwrap();

    // The extend Step's InsertText was interrupted mid-typewriter, leaving
    // a partial prefix of the tail comment in the workspace.
    let report = run_recovery_scenario(
        source.path(),
        ScenarioMode::ByStep,
        crashed_artifacts.path(),
        &["write"],
        "extend",
        "fn main() {\n    // temporary\n    println!(\"old\");\n}\n// ta",
    )
    .await;

    assert_recovery_matches_reference(&report, &reference);
}

#[tokio::test]
async fn crashed_insert_recovers_identically_in_continuous_learning() {
    let source = tempfile::tempdir().unwrap();
    recovery_self_heal_course(source.path());
    let reference_artifacts = tempfile::tempdir().unwrap();
    let reference = uninterrupted_recovery_reference(
        source.path(),
        ScenarioMode::Continuous,
        reference_artifacts.path(),
    )
    .await;
    let crashed_artifacts = tempfile::tempdir().unwrap();

    let report = run_recovery_scenario(
        source.path(),
        ScenarioMode::Continuous,
        crashed_artifacts.path(),
        &["write"],
        "extend",
        "fn main() {\n    // temporary\n    println!(\"old\");\n}\n// ta",
    )
    .await;

    assert_recovery_matches_reference(&report, &reference);
}

#[tokio::test]
async fn crashed_range_deletion_recovers_to_the_uninterrupted_workspace() {
    let source = tempfile::tempdir().unwrap();
    recovery_self_heal_course(source.path());
    let reference_artifacts = tempfile::tempdir().unwrap();
    let reference = uninterrupted_recovery_reference(
        source.path(),
        ScenarioMode::ByStep,
        reference_artifacts.path(),
    )
    .await;
    let crashed_artifacts = tempfile::tempdir().unwrap();

    // The cleanup Step's DeleteText was interrupted with a remnant of the
    // expected text still in the range.
    let report = run_recovery_scenario(
        source.path(),
        ScenarioMode::ByStep,
        crashed_artifacts.path(),
        &["write", "extend"],
        "cleanup",
        "fn main() {\n    // te    println!(\"old\");\n}\n// tail\n",
    )
    .await;

    assert_recovery_matches_reference(&report, &reference);
}

#[tokio::test]
async fn crashed_replacement_recovers_to_the_uninterrupted_workspace() {
    let source = tempfile::tempdir().unwrap();
    recovery_self_heal_course(source.path());
    let reference_artifacts = tempfile::tempdir().unwrap();
    let reference = uninterrupted_recovery_reference(
        source.path(),
        ScenarioMode::ByStep,
        reference_artifacts.path(),
    )
    .await;
    let crashed_artifacts = tempfile::tempdir().unwrap();

    // The polish Step's ReplaceText was interrupted while typing, leaving a
    // prefix of the replacement code inside the range.
    let report = run_recovery_scenario(
        source.path(),
        ScenarioMode::ByStep,
        crashed_artifacts.path(),
        &["write", "extend", "cleanup"],
        "polish",
        "fn main() {\n    println!(\"ne;\n}\n// tail\n",
    )
    .await;

    assert_recovery_matches_reference(&report, &reference);
}

fn recovery_line_break_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "recovery_line_break",
            "title": "Recovery Line Break",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Chapter",
                "children": [{
                    "id": "recover",
                    "title": "Recover",
                    "steps": [
                        { "step_file": "steps/write.json" },
                        { "step_file": "steps/cleanup.json" }
                    ]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let steps = [
        (
            "write",
            serde_json::json!([
                { "type": "CreateFile", "path": "main.rs" },
                { "type": "OpenFile", "path": "main.rs" },
                {
                    "type": "InsertText",
                    "path": "main.rs",
                    "target": {"line": 1},
                    "text": "alpha\nkeep one\nbeta\n"
                }
            ]),
        ),
        (
            "cleanup",
            serde_json::json!([
                { "type": "OpenFile", "path": "main.rs" },
                {
                    "type": "DeleteText",
                    "path": "main.rs",
                    "target": {
                        "start": { "line": 2 },
                        "end": { "line": 2 },
                        "expected_text": "keep one\n"
                    },
                    "direction": "delete"
                }
            ]),
        ),
    ];
    for (step_id, actions) in steps {
        fs::write(
            root.join("steps").join(format!("{step_id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "step_id": step_id,
                "title": step_id,
                "actions": actions
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn crashed_deletion_with_included_line_break_recovers_to_the_uninterrupted_workspace() {
    let source = tempfile::tempdir().unwrap();
    recovery_line_break_course(source.path());
    let reference_artifacts = tempfile::tempdir().unwrap();
    let reference = run_scenario(ScenarioSpec {
        course_path: source.path().to_path_buf(),
        unit_path: Some(format!("{}/{}", "ch", "recover")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: reference_artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(3),
    })
    .await
    .unwrap();
    let crashed_artifacts = tempfile::tempdir().unwrap();

    // The cleanup Step's forward deletion consumed a prefix of the line but
    // not yet its included end line break: the remnant plus the break must
    // still converge to the uninterrupted terminal workspace. Coordinate
    // end anchors only stay resolvable while the break is present; once a
    // backspace deletion consumed it the line structure collapses and the
    // course author must fall back to text anchors (safe-replay rule).
    let report = run_scenario_with_setup(
        ScenarioSpec {
            course_path: source.path().to_path_buf(),
            unit_path: Some(format!("{}/{}", "ch", "recover")),
            mode: ScenarioMode::ByStep,
            adapter_script: AdapterScript::acknowledge_all(),
            adapter_script_path: None,
            artifact_root: crashed_artifacts.path().to_path_buf(),
            response_timeout: Duration::from_secs(3),
        },
        move |session: &mut CourseSession| {
            let unit_path = format!("{}/{}", "ch", "recover");
            session.start_unit(&unit_path, true)?;
            session.record_progress(&unit_path, "write", true)?;
            session.record_progress(&unit_path, "cleanup", false)?;
            let workspace = session
                .active_workspace()
                .expect("the unit workspace is prepared");
            fs::write(
                workspace.join(&unit_path).join("main.rs"),
                "alpha\none\nbeta\n",
            )?;
            Ok(())
        },
    )
    .await
    .unwrap();

    assert_recovery_matches_reference(&report, &reference);
}
