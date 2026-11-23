use super::super::*;
use super::support::{copy_directory, fixture_path, stored_path};
use std::path::PathBuf;

fn narration_fixture() -> PathBuf {
    fixture_path("narration-demo")
}

fn action_before_narration_fixture(root: &std::path::Path) -> PathBuf {
    let package = root.join("action-before");
    std::fs::create_dir_all(package.join("steps")).unwrap();
    std::fs::create_dir_all(package.join("assets/narration")).unwrap();
    std::fs::create_dir_all(package.join("workspace")).unwrap();
    std::fs::write(package.join("workspace/.gitkeep"), "").unwrap();
    std::fs::copy(
        fixture_path("phase-timeline-demo").join("assets/narration/phases.ogg"),
        package.join("assets/narration/before.ogg"),
    )
    .unwrap();
    std::fs::write(
        package.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "action_before_narration",
            "title": "Action Before Narration",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "assets": [{
                        "id": "narration",
                        "path": "assets/narration/before.ogg"
                    }],
                    "steps": [{"step_file": "steps/write.json"}]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        package.join("steps/write.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "write",
            "title": "Write",
            "actions": [{
                "type": "CreateFile",
                "path": "hello.txt",
                "narration": {
                    "before": {
                        "asset_id": "narration",
                        "clips": [{
                            "offset_ms": 250,
                            "duration_ms": 1_500,
                            "caption": "先听讲解。"
                        }]
                    }
                }
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    package
}

#[test]
fn action_before_narration_is_available_from_the_package() {
    let directory = tempfile::tempdir().unwrap();
    let package = action_before_narration_fixture(directory.path());
    let db_path = directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(&package).unwrap();
    session.start_first_unit(true).unwrap();
    let package = package.canonicalize().unwrap();

    let asset = session
        .action_narration_audio("ch/unit", "write", 0, ActionNarrationPhase::Before, 0)
        .unwrap()
        .expect("Action Before Clip asset");
    let audio_path = PathBuf::from(asset.audio_path.as_deref().unwrap())
        .canonicalize()
        .unwrap();
    assert!(audio_path.starts_with(&package));
    assert!(audio_path.is_file());
    assert_eq!(asset.offset_ms, Some(250));
    assert_eq!(asset.duration_ms, 1_500);
}

#[test]
fn action_during_and_after_assets_are_available_from_the_package() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_directory(&fixture_path("phase-timeline-demo"), &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    for (unit_id, phase, expected_caption) in [
        ("during", ActionNarrationPhase::During, "During clip"),
        ("after", ActionNarrationPhase::After, "After clip"),
    ] {
        let unit_path = format!("phases/{unit_id}");
        session.start_unit(&unit_path, true).unwrap();
        let step_id = unit_id;
        let asset = session
            .action_narration_audio(&unit_path, step_id, 0, phase, 0)
            .unwrap()
            .expect("Action narration asset");
        assert_eq!(asset.caption.as_deref(), Some(expected_caption));
        assert!(PathBuf::from(asset.audio_path.unwrap()).is_file());
    }
}

#[test]
fn reset_scope_uses_the_unit_workspace_path_even_when_it_is_missing() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_directory(&narration_fixture(), &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_first_unit(true).unwrap();

    let unit_workspace = package.join("workspace/ch_narration/sec_narration");
    std::fs::remove_dir_all(&unit_workspace).unwrap();
    assert_eq!(
        session
            .reset_scope_for_unit("ch_narration/sec_narration")
            .unwrap()
            .workspace_paths,
        vec!["ch_narration/sec_narration"]
    );

    std::fs::create_dir_all(&unit_workspace).unwrap();
    std::fs::write(unit_workspace.join("intro.txt"), "").unwrap();
    assert_eq!(
        session.active_workspace(),
        Some(stored_path(&package).join("workspace"))
    );
    assert_eq!(
        session
            .reset_scope_for_unit("ch_narration/sec_narration")
            .unwrap()
            .workspace_paths,
        vec!["ch_narration/sec_narration"]
    );
}
