use super::super::*;
use super::support::copy_directory;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    super::support::fixture_path(name)
}

#[test]
fn starting_a_unit_after_traversal_keeps_other_progress() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_directory(&fixture("independent-units"), &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    for (unit_path, step_id) in [
        ("ch/unit_a", "shared_1"),
        ("ch/unit_b", "shared_1_node_unit_b"),
        ("ch/unit_c", "shared_1_node_unit_c"),
        ("ch/unit_d", "shared_1_node_unit_d"),
        ("ch/unit_e", "shared_1_node_unit_e"),
    ] {
        session.start_unit(unit_path, true).unwrap();
        std::fs::write(
            package.join("workspace").join(unit_path).join("marker.txt"),
            "old",
        )
        .unwrap();
        session.record_progress(unit_path, step_id, true).unwrap();
    }

    let course = session
        .start_unit("ch/unit_c", true)
        .unwrap()
        .active_course
        .unwrap();

    assert_eq!(
        find_unit(&course.tree, "ch/unit_c").unwrap().status,
        UnitStatus::InProgress
    );
    assert!(!package.join("workspace/ch/unit_c/marker.txt").exists());
    for unit_path in ["ch/unit_a", "ch/unit_b", "ch/unit_d", "ch/unit_e"] {
        let unit = find_unit(&course.tree, unit_path).unwrap();
        assert_eq!(
            unit.status,
            UnitStatus::Completed,
            "{unit_path} progress must survive starting again after traversal"
        );
        assert!(
            unit.steps.iter().all(|step| step.completed),
            "{unit_path} Step progress must survive starting again after traversal"
        );
        assert!(
            package
                .join("workspace")
                .join(unit_path)
                .join("marker.txt")
                .is_file(),
            "{unit_path} workspace should survive a new round"
        );
    }
}

#[test]
fn resetting_one_unit_preserves_other_unit_states() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_directory(&fixture("independent-units"), &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit_c", true).unwrap();
    session
        .record_progress("ch/unit_c", "shared_1_node_unit_c", true)
        .unwrap();
    session.start_unit("ch/unit_d", true).unwrap();

    let outcome = session.reset_unit("ch/unit_c", true).unwrap();
    let course = outcome.state.active_course.unwrap();

    assert_eq!(
        find_unit(&course.tree, "ch/unit_c").unwrap().status,
        UnitStatus::NotStarted
    );
    assert_eq!(
        find_unit(&course.tree, "ch/unit_d").unwrap().status,
        UnitStatus::NotStarted
    );
}

#[test]
fn resetting_refreshes_a_stale_progress_cache() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_directory(&fixture("independent-units"), &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit_c", true).unwrap();
    session
        .record_progress("ch/unit_c", "shared_1_node_unit_c", true)
        .unwrap();
    session.progress = None;

    let outcome = session.reset_unit("ch/unit_c", true).unwrap();

    let course = outcome.state.active_course.unwrap();
    assert!(!course.has_progress());
}

#[test]
fn course_reset_clears_all_units() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_directory(&fixture("independent-units"), &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit_e", true).unwrap();
    session
        .record_progress("ch/unit_e", "shared_1_node_unit_e", true)
        .unwrap();

    let state = session.reset_course(true).unwrap();
    let course = state.active_course.unwrap();

    assert!(!course.has_progress());
    assert!(
        course
            .tree
            .iter()
            .flat_map(|node| &node.children)
            .filter_map(|node| node.unit.as_ref())
            .all(|unit| unit.status == UnitStatus::NotStarted)
    );
}

#[test]
fn course_reset_refuses_a_workspace_that_would_delete_the_package() {
    for workspace in [".", "steps"] {
        let directory = tempfile::tempdir().unwrap();
        let package = directory.path().join("course");
        copy_directory(&fixture("independent-units"), &package);
        let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
        session.open_course(&package).unwrap();

        let manifest_path = package.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["workspace"] = serde_json::json!(workspace);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let error = session
            .reset_course(true)
            .map(|_| ())
            .expect_err("reset must reject the workspace")
            .to_string();

        assert!(error.contains("workspace"), "{workspace}: {error}");
        assert!(
            manifest_path.is_file(),
            "workspace {workspace:?} let Course Reset delete manifest.json"
        );
        assert!(
            package.join("steps").is_dir(),
            "workspace {workspace:?} let Course Reset delete steps/"
        );
    }
}
