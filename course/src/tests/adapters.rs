use super::*;

#[test]
fn presentation_fixture_uses_a_side_effect_free_present_action() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/presentation-demo");
    let course = load_package(&root).unwrap();
    let playback = course.playback_steps();
    let actions = course.actions();

    assert!(matches!(actions.as_slice(), [Action::Present]));
    let clip = &playback[0].actions[0]
        .narration
        .before
        .as_ref()
        .expect("Present Before narration")
        .clips[0];
    assert_eq!(clip.duration_ms, 2_500);
    assert!(clip.audio_path.is_none());
    assert_eq!(
        clip.caption.as_deref(),
        Some("纯讲解也可以完全由内联字幕和 Action 旁白阶段承载。")
    );
    assert_eq!(
        serde_json::to_value(Action::Present).unwrap(),
        serde_json::json!({"type": "Present"})
    );
}

#[test]
fn units_report_when_their_actions_require_an_app_adapter() {
    let presentation_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/presentation-demo");
    let presentation = load_package(&presentation_root).unwrap();
    assert!(!presentation.unit_requires_app_adapter("ch_presentation/pure_presentation"));

    let filesystem_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/multi-step-demo");
    let filesystem = load_package(&filesystem_root).unwrap();
    assert!(filesystem.unit_requires_app_adapter("ch_multi_step/main"));
}

#[test]
fn jupyterlab_fixture_loads_with_a_jupyterlab_requirement() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/jupyterlab-demo");
    let course = load_package(&root).unwrap();

    assert_eq!(course.tree.adapters, vec![AdapterId::Jupyterlab]);
    assert_eq!(
        course.effective_adapters_for_path("ch_jupyterlab/hello_notebook"),
        vec![AdapterId::Jupyterlab]
    );
    assert!(course.unit_requires_app_adapter("ch_jupyterlab/hello_notebook"));
}

#[test]
fn empty_adapters_are_invalid_for_external_actions_or_environment_checks() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert("adapters".to_string(), serde_json::json!([]));
    unit.insert("environment".to_string(), serde_json::json!([]));
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("course.adapter_configuration"));
    assert!(error.contains("ch/main"));
    assert!(error.contains("non-Present"));
}

#[test]
fn present_only_units_can_disable_adapters_without_environment_checks() {
    let present_step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{"type": "Present"}]
    });
    let package = write_test_package("steps/present.json", present_step);
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert("adapters".to_string(), serde_json::json!([]));
    unit.insert("environment".to_string(), serde_json::json!([]));
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let course = load_package(package.path()).unwrap();

    assert!(!course.unit_has_invalid_adapter_configuration("ch/main"));
}

#[test]
fn environment_checks_require_an_app_adapter() {
    let present_step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{"type": "Present"}]
    });
    let package = write_test_package("steps/present.json", present_step);
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert("adapters".to_string(), serde_json::json!([]));
    unit.insert(
        "environment".to_string(),
        serde_json::json!([{
            "id": "rustc",
            "command": "rustc --version"
        }]),
    );
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("course.adapter_configuration"));
    assert!(error.contains("ch/main"));
    assert!(error.contains("environment checks (rustc)"));
}
