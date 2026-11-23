use super::*;

#[test]
fn scenario_cli_prints_a_report_for_an_agent() {
    let artifacts = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cynapse-scenario"))
        .arg("--course")
        .arg(fixture("multi-step-demo"))
        .arg("--unit")
        .arg("ch_multi_step/main")
        .arg("--artifact-root")
        .arg(artifacts.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "completed");
    assert!(
        report["repro_command"]
            .as_str()
            .unwrap()
            .contains("--unit \"ch_multi_step/main\"")
    );
    assert!(
        report["events"].as_array().unwrap().iter().any(|event| {
            event["detail"]["unit_path"] == serde_json::json!("ch_multi_step/main")
        })
    );
    assert!(
        report["workspace_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "ch_multi_step/main/multi_step_demo.txt")
    );
    assert!(report["correlation_id"].as_str().is_some());
}

#[test]
fn scenario_cli_rejects_other_target_selectors_with_a_unit_hint() {
    let artifacts = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cynapse-scenario"))
        .arg("--course")
        .arg(fixture("flexible-tree-demo"))
        .arg("--target")
        .arg("ch01_basics")
        .arg("--artifact-root")
        .arg(artifacts.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("use --unit <node path>"));
}

#[test]
fn scenario_cli_requires_a_value_after_unit() {
    let artifacts = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cynapse-scenario"))
        .arg("--course")
        .arg(fixture("flexible-tree-demo"))
        .arg("--unit")
        .arg("--artifact-root")
        .arg(artifacts.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--unit requires a node path"));
}

#[test]
fn scenario_cli_accepts_a_root_unit_path() {
    let artifacts = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cynapse-scenario"))
        .arg("--course")
        .arg(fixture("flexible-tree-demo"))
        .arg("--unit")
        .arg("intro")
        .arg("--artifact-root")
        .arg(artifacts.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| { event["detail"]["unit_path"] == serde_json::json!("intro") })
    );
}

#[test]
fn scenario_cli_defaults_to_the_first_unit_path() {
    let artifacts = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cynapse-scenario"))
        .arg("--course")
        .arg(fixture("multi-step-demo"))
        .arg("--artifact-root")
        .arg(artifacts.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["events"].as_array().unwrap().iter().any(|event| {
            event["detail"]["unit_path"] == serde_json::json!("ch_multi_step/main")
        })
    );
}

#[test]
fn continuous_scenario_cli_reports_and_reproduces_with_the_unit_path() {
    let artifacts = tempfile::tempdir().unwrap();
    let unit_path = "ch02_advanced/ch02_02_abstraction/structural_role";
    let output = Command::new(env!("CARGO_BIN_EXE_cynapse-scenario"))
        .arg("--course")
        .arg(fixture("flexible-tree-demo"))
        .arg("--unit")
        .arg(unit_path)
        .arg("--mode")
        .arg("continuous")
        .arg("--artifact-root")
        .arg(artifacts.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["repro_command"]
            .as_str()
            .unwrap()
            .contains(&format!("--unit \"{unit_path}\""))
    );
    assert!(
        report["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| { event["detail"]["unit_path"] == serde_json::json!(unit_path) })
    );
}
