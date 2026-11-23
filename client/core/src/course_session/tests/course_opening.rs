use super::super::*;
use super::support::{
    assert_open_state_unchanged, copied_fixture, copy_directory, fixture_path, stored_path,
};
use rusqlite::OptionalExtension;

#[test]
fn first_launch_has_no_active_course_or_recent_courses() {
    let directory = tempfile::tempdir().unwrap();
    let session = CourseSession::new(directory.path().join("state.db")).unwrap();

    let state = session.state();

    assert!(state.active_course.is_none());
    assert!(state.recent_courses.is_empty());
}

#[test]
fn recent_courses_are_capped_at_five() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let fixture = fixture_path("multi-step-demo");
    let mut session = CourseSession::new(&db_path).unwrap();

    for index in 0..6 {
        let package = directory.path().join(format!("course-{index}"));
        copy_directory(&fixture, &package);
        session.open_course(&package).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let state = session.state();
    assert_eq!(state.recent_courses.len(), 5);
    assert_eq!(
        Path::new(&state.recent_courses[0].path),
        stored_path(directory.path().join("course-5")).as_path()
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let stored: i64 = connection
        .query_row("SELECT COUNT(*) FROM recent_courses", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, 5);
}

#[test]
fn clearing_recent_courses_keeps_the_active_course() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let db_path = directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(&fixture).unwrap();

    let state = session.clear_recent_courses().unwrap();

    assert!(state.recent_courses.is_empty());
    assert_eq!(
        state
            .active_course
            .as_ref()
            .map(|course| course.id.as_str()),
        Some("multi_step_demo")
    );
    drop(session);

    let restored = CourseSession::new(&db_path).unwrap().state();
    assert!(restored.recent_courses.is_empty());
    assert_eq!(
        restored
            .active_course
            .as_ref()
            .map(|course| course.id.as_str()),
        Some("multi_step_demo")
    );
}

#[test]
fn a_newer_schema_is_refused_without_dropping_data() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    {
        let connection = rusqlite::Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE marker (value TEXT NOT NULL);
                 INSERT INTO marker VALUES ('keep');
                 PRAGMA user_version = 13;",
            )
            .unwrap();
    }

    let error = match CourseSession::new(db_path.clone()) {
        Ok(_) => panic!("a newer schema must not open"),
        Err(error) => error,
    };

    assert!(
        matches!(error, CourseSessionError::SchemaTooNew(13)),
        "{error:?}"
    );
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let value: String = connection
        .query_row("SELECT value FROM marker", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "keep");
}

#[test]
fn opening_a_course_sets_the_active_course_and_recent_course() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    let state = session.open_course(&fixture).unwrap();

    let active = state.active_course.expect("active course");
    assert_eq!(active.id, "multi_step_demo");
    assert_eq!(active.title, "Step Demo");
    assert_eq!(active.tree.len(), 1);
    assert_eq!(active.tree[0].children.len(), 1);
    assert_eq!(
        active.tree[0].children[0]
            .unit
            .as_ref()
            .unwrap()
            .steps
            .len(),
        2
    );
    assert_eq!(state.recent_courses.len(), 1);
    assert!(
        state
            .active_course_path
            .as_deref()
            .is_some_and(|path| Path::new(path) == stored_path(&fixture))
    );
    assert_eq!(active.tree.len(), 1);
    assert_eq!(active.tree[0].title, "Step Demo");
}

#[test]
fn opening_a_course_selects_the_first_unit_when_nothing_is_selected() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    let state = session.open_course(&fixture).unwrap();

    assert_eq!(
        state.selected_unit.as_ref().map(|unit| unit.path.as_str()),
        Some("ch_multi_step/main")
    );
}

#[test]
fn opening_a_course_creates_its_declared_workspace_root() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    std::fs::remove_dir_all(package.join("workspace")).unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    session.open_course(&package).unwrap();

    assert!(package.join("workspace").is_dir());
    assert_eq!(
        session.active_workspace(),
        Some(stored_path(&package).join("workspace"))
    );
}

#[test]
fn restoring_a_course_recreates_its_declared_workspace_root() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    let db_path = directory.path().join("state.db");
    {
        let mut session = CourseSession::new(&db_path).unwrap();
        session.open_course(&package).unwrap();
    }
    std::fs::remove_dir_all(package.join("workspace")).unwrap();

    let session = CourseSession::new(&db_path).unwrap();

    assert!(package.join("workspace").is_dir());
    assert_eq!(
        session.active_workspace(),
        Some(stored_path(&package).join("workspace"))
    );
}

#[test]
fn active_workspace_is_cached_after_opening() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    session.open_course(&package).unwrap();
    let workspace = session.active_workspace();
    assert!(workspace.is_some());

    // The cached path no longer depends on reading the package on every call.
    std::fs::remove_file(package.join("manifest.json")).unwrap();
    assert_eq!(session.active_workspace(), workspace);

    session.clear_active_course_in_memory();
    assert_eq!(session.active_workspace(), None);
}

#[test]
fn reopening_after_a_version_change_clears_progress() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    let db_path = directory.path().join("state.db");

    {
        let mut session = CourseSession::new(&db_path).unwrap();
        session.open_course(&package).unwrap();
        session.start_first_unit(false).unwrap();
    }
    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!("9.9.9");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let restored = CourseSession::new(&db_path).unwrap();
    let state = restored.state();
    let active = state.active_course.expect("active course");
    assert!(!active.has_progress());
    assert!(active.current_unit_path.is_none());
    assert!(restored.progress.is_none());
}

#[test]
fn opening_a_tree_course_exposes_recursive_nodes_and_unit_paths() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = fixture_path("flexible-tree-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    let state = session.open_course(&fixture).unwrap();
    let active = state.active_course.expect("active tree course");

    assert_eq!(
        state.selected_unit.as_ref().map(|unit| unit.path.as_str()),
        Some("intro")
    );
    assert_eq!(
        active
            .tree
            .iter()
            .map(|node| node.title.as_str())
            .collect::<Vec<_>>(),
        vec!["简介", "第一章 基础", "第二章 进阶", "附录"]
    );
    assert_eq!(
        active.tree[0].unit.as_ref().map(|unit| unit.path.as_str()),
        Some("intro")
    );
    assert!(active.tree[1].unit.is_none());
    assert_eq!(
        active.tree[1]
            .children
            .iter()
            .map(|node| node.unit.as_ref().map(|unit| unit.path.as_str()))
            .collect::<Vec<_>>(),
        vec![
            Some("ch01_basics/what_is_node"),
            Some("ch01_basics/tree_and_recursion"),
        ]
    );
    assert_eq!(
        active.tree[2].children[1].children[0]
            .unit
            .as_ref()
            .map(|unit| unit.path.as_str()),
        Some("ch02_advanced/ch02_02_abstraction/structural_role")
    );

    let wire = serde_json::to_value(&active).unwrap();
    assert!(wire["tree"][0]["unit"].is_object());
    assert_eq!(wire["tree"][0]["title"], "简介");
    assert_eq!(wire["tree"][0]["unit"]["path"], "intro");
    assert!(wire["tree"][0]["children"].is_array());
    assert!(wire["tree"][0].get("id").is_none());
    assert!(wire["tree"][1].get("unit").is_none());
}

#[test]
fn reopening_the_session_restores_the_active_course() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let db_path = directory.path().join("state.db");
    {
        let mut session = CourseSession::new(&db_path).unwrap();
        session.open_course(&fixture).unwrap();
    }

    let restored = CourseSession::new(&db_path).unwrap().state();

    let active = restored.active_course.expect("restored active course");
    assert_eq!(active.id, "multi_step_demo");
    assert_eq!(restored.recent_courses.len(), 1);
    assert_eq!(restored.recent_courses[0].id, "multi_step_demo");
}

#[test]
fn clearing_active_course_in_memory_preserves_recent_and_persisted_state() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let db_path = directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(&fixture).unwrap();
    session.start_first_unit(false).unwrap();

    let cleared = session.clear_active_course_in_memory();

    assert!(cleared.active_course.is_none());
    assert!(cleared.active_course_path.is_none());
    assert!(cleared.selected_unit.is_none());
    assert_eq!(cleared.recent_courses.len(), 1);
    assert_eq!(cleared.recent_courses[0].id, "multi_step_demo");

    drop(session);
    let restored = CourseSession::new(&db_path).unwrap().state();
    assert_eq!(
        restored
            .active_course
            .as_ref()
            .map(|course| course.id.as_str()),
        Some("multi_step_demo")
    );
    assert!(
        restored
            .active_course
            .as_ref()
            .is_some_and(|course| course.current_unit_path.is_some())
    );
}

#[test]
fn missing_recent_courses_are_pruned_from_the_database() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("temporary-course");
    copy_directory(&fixture, &package);
    let db_path = directory.path().join("state.db");
    {
        let mut session = CourseSession::new(&db_path).unwrap();
        session.open_course(&package).unwrap();
    }
    std::fs::remove_dir_all(&package).unwrap();

    let restored = CourseSession::new(&db_path).unwrap();
    let state = restored.state();
    assert!(state.active_course.is_none());
    assert!(state.recent_courses.is_empty());

    drop(restored);
    let restored = CourseSession::new(&db_path).unwrap();
    assert!(restored.state().recent_courses.is_empty());
}

#[test]
fn opening_another_course_replaces_the_active_course_and_keeps_recents() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, multi_step) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    session.open_course(multi_step).unwrap();
    let state = session.open_course(fixture_path("rust-hello")).unwrap();

    assert_eq!(
        state
            .active_course
            .as_ref()
            .map(|course| course.id.as_str()),
        Some("rust_hello_world")
    );
    assert_eq!(state.recent_courses.len(), 2);
    assert_eq!(state.recent_courses[0].id, "rust_hello_world");
    assert_eq!(state.recent_courses[1].id, "multi_step_demo");
}

#[test]
fn reopening_a_course_restores_its_progress() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, multi_step) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(multi_step.clone()).unwrap();
    session.start_first_unit(false).unwrap();

    session.open_course(fixture_path("rust-hello")).unwrap();
    let reopened = session.open_course(multi_step).unwrap();
    let course = reopened.active_course.unwrap();

    assert!(course.current_unit_path.is_some());
}

#[test]
fn reopening_a_closed_course_does_not_resurrect_selection_from_the_resume_cursor() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("independent-units");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let opened = session.open_course(&fixture).unwrap();
    let unit_path = opened.selected_unit.expect("first unit").path;
    session.start_unit(&unit_path, true).unwrap();
    session.close_course().unwrap();

    let reopened = session.open_course(&fixture).unwrap();

    assert!(
        reopened.selected_unit.is_none(),
        "closing clears the explicit selection; the cursor must not select it again"
    );
    let course = reopened.active_course.expect("the course reopens");
    assert_eq!(
        course.current_unit_path.as_deref(),
        Some(unit_path.as_str()),
        "the resume cursor still records where the learner stopped"
    );
}

#[test]
fn reopening_a_session_restores_the_selected_unit_and_step_cursor() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    let db_path = directory.path().join("state.db");
    let selected_path;
    {
        let mut session = CourseSession::new(&db_path).unwrap();
        session.open_course(&package).unwrap();
        let started = session.start_first_unit(true).unwrap();
        selected_path = started.selected_unit.unwrap().path;
    }

    let restored = CourseSession::new(&db_path).unwrap().state();
    assert_eq!(
        restored
            .selected_unit
            .as_ref()
            .map(|unit| unit.path.as_str()),
        Some(selected_path.as_str())
    );
    let course = restored.active_course.expect("active course");
    assert_eq!(
        course.current_unit_path.as_deref(),
        Some(selected_path.as_str())
    );
    assert!(course.current_step_id.is_some());
    assert_eq!(
        restored.selected_unit.and_then(|unit| unit.current_step_id),
        course.current_step_id
    );
}

#[test]
fn reopening_an_edited_package_uses_the_new_tree_and_keeps_progress() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_first_unit(false).unwrap();

    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["children"].as_array_mut().unwrap().insert(
        0,
        serde_json::json!({
            "id": "ch_intro",
            "title": "Introduction",
            "children": [{
                "id": "intro",
                "title": "Introduction",
                "steps": [{ "step_file": "steps/write_first.json" }]
            }]
        }),
    );
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let reopened = session.open_course(&package).unwrap();
    let active = reopened.active_course.expect("active course");

    assert_eq!(active.tree.len(), 2);
    assert_eq!(active.tree[0].title, "Introduction");
    assert_eq!(active.version, "1.0.0");
    assert!(active.current_unit_path.is_some());
}

#[test]
fn reopening_a_new_course_version_starts_fresh() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_first_unit(false).unwrap();

    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!("1.1.0");
    manifest["children"].as_array_mut().unwrap().insert(
        0,
        serde_json::json!({
            "id": "ch_intro",
            "title": "Introduction",
            "children": [{
                "id": "intro",
                "title": "Introduction",
                "steps": [{ "step_file": "steps/write_first.json" }]
            }]
        }),
    );
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let reopened = session.open_course(&package).unwrap();
    let active = reopened.active_course.expect("active course");

    assert_eq!(active.version, "1.1.0");
    assert_eq!(active.tree.len(), 2);
    assert_eq!(active.tree[0].title, "Introduction");
    assert!(!active.has_progress());
    let started = session.select_unit("ch_intro/intro").unwrap();
    let course = started.active_course.expect("started course");
    assert_eq!(course.current_unit_path.as_deref(), Some("ch_intro/intro"));
}

#[test]
fn restarting_after_a_course_version_change_starts_fresh() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let package = directory.path().join("course");
    let db_path = directory.path().join("state.db");
    copy_directory(&fixture, &package);
    {
        let mut session = CourseSession::new(&db_path).unwrap();
        session.open_course(&package).unwrap();
        session.start_first_unit(false).unwrap();
    }

    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!("1.1.0");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let restored = CourseSession::new(&db_path).unwrap().state();
    let active = restored.active_course.expect("active course");

    assert_eq!(active.version, "1.1.0");
    assert!(!active.has_progress());
}

#[test]
fn failed_open_keeps_the_previous_active_course() {
    let directory = tempfile::tempdir().unwrap();
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&fixture).unwrap();

    assert!(
        session
            .open_course(directory.path().join("missing-course"))
            .is_err()
    );
    let state = session.state();
    assert_eq!(
        state
            .active_course
            .as_ref()
            .map(|course| course.id.as_str()),
        Some("multi_step_demo")
    );
    assert_eq!(state.recent_courses.len(), 1);
}

#[test]
fn failed_resource_preparation_is_a_complete_open_no_op() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (_old_package, old_course) = copied_fixture("rust-hello");
    let (_target_package, target_course) = copied_fixture("multi-step-demo");
    let mut session = CourseSession::new(&db_path).unwrap();

    let target = session.open_course(&target_course).unwrap();
    let target_id = target.active_course.as_ref().unwrap().id.clone();
    let target_unit = target.selected_unit.as_ref().unwrap().path.clone();
    session.start_unit(&target_unit, true).unwrap();
    session.open_course(&old_course).unwrap();
    let before = session.state();
    let old_path = before.active_course_path.clone();

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let progress_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(progress_before > 0);
    drop(connection);

    session.fail_next_open(LifecycleFailpoint::Resources);
    assert!(session.open_course(&target_course).is_err());
    assert_open_state_unchanged(&before, &session.state());

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let persisted_path: String = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_course_path'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(Some(persisted_path), old_path);
    let persisted_unit: Option<String> = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_unit_path'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(
        persisted_unit,
        before.selected_unit.as_ref().map(|unit| unit.path.clone())
    );
    let progress_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(progress_after, progress_before);
}

#[test]
fn failed_open_does_not_clear_versioned_progress() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (_old_package, old_course) = copied_fixture("rust-hello");
    let (_target_package, target_course) = copied_fixture("multi-step-demo");
    let (_newer_package, newer_course) = copied_fixture("multi-step-demo");
    let manifest_path = newer_course.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!("1.1.0");
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let mut session = CourseSession::new(&db_path).unwrap();
    let target = session.open_course(&target_course).unwrap();
    let target_id = target.active_course.as_ref().unwrap().id.clone();
    let target_unit = target.selected_unit.as_ref().unwrap().path.clone();
    session.start_unit(&target_unit, true).unwrap();
    session.open_course(&old_course).unwrap();
    let before = session.state();
    let old_path = before.active_course_path.clone();

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let progress_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(progress_before > 0);
    drop(connection);

    session.fail_next_open(LifecycleFailpoint::Commit);
    assert!(session.open_course(&newer_course).is_err());
    assert_open_state_unchanged(&before, &session.state());

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let persisted_path: String = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_course_path'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(Some(persisted_path), old_path);
    let persisted_unit: Option<String> = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_unit_path'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(
        persisted_unit,
        before.selected_unit.as_ref().map(|unit| unit.path.clone())
    );
    let progress_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(progress_after, progress_before);
}

#[test]
fn failed_restore_resource_preparation_is_atomic() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (_package, fixture) = copied_fixture("multi-step-demo");
    let mut setup = CourseSession::new(&db_path).unwrap();
    let state = setup.open_course(&fixture).unwrap();
    let target_id = state.active_course.as_ref().unwrap().id.clone();
    let target_unit = state.selected_unit.as_ref().unwrap().path.clone();
    let old_path = state.active_course_path.clone();
    setup.start_unit(&target_unit, true).unwrap();
    drop(setup);

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let progress_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(progress_before > 0);
    drop(connection);

    let mut session = CourseSession::create_unrestored(&db_path).unwrap();
    session.fail_next_restore(LifecycleFailpoint::Resources);
    assert!(session.restore().is_err());
    assert!(session.state().active_course.is_none());

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let persisted_path: String = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_course_path'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(Some(persisted_path), old_path);
    let progress_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(progress_after, progress_before);

    let restored = CourseSession::new(&db_path).unwrap().state();
    assert_eq!(
        restored
            .active_course
            .as_ref()
            .map(|course| course.id.as_str()),
        Some(target_id.as_str())
    );
}

#[test]
fn failed_restore_commit_preserves_versioned_progress() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let (_old_package, target_course) = copied_fixture("multi-step-demo");
    let (_newer_package, newer_course) = copied_fixture("multi-step-demo");
    let manifest_path = newer_course.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!("1.1.0");
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let mut setup = CourseSession::new(&db_path).unwrap();
    let state = setup.open_course(&target_course).unwrap();
    let target_id = state.active_course.as_ref().unwrap().id.clone();
    let target_unit = state.selected_unit.as_ref().unwrap().path.clone();
    setup.start_unit(&target_unit, true).unwrap();
    drop(setup);

    let newer_path = stored_path(&newer_course).to_string_lossy().to_string();
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute(
            "UPDATE settings SET value = ?1 WHERE key = 'active_course_path'",
            [&newer_path],
        )
        .unwrap();
    let progress_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(progress_before > 0);
    drop(connection);

    let mut session = CourseSession::create_unrestored(&db_path).unwrap();
    session.fail_next_restore(LifecycleFailpoint::Commit);
    assert!(session.restore().is_err());
    assert!(session.state().active_course.is_none());

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let persisted_path: String = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_course_path'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted_path, newer_path);
    let persisted_unit: Option<String> = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_unit_path'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(persisted_unit, Some(target_unit));
    let progress_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(progress_after, progress_before);
    drop(connection);

    let recovered = CourseSession::new(&db_path).unwrap();
    let recovered_state = recovered.state();
    assert_eq!(
        recovered_state
            .active_course
            .as_ref()
            .map(|course| course.version.as_str()),
        Some("1.1.0")
    );
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let progress_after_success: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM course_progress WHERE id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(progress_after_success, 0);
}

#[test]
fn a_new_database_creates_the_final_schema_without_course_status() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let _session = CourseSession::new(&db_path).unwrap();

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 12);
    let columns = connection
        .prepare("SELECT name FROM pragma_table_info('course_progress')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!columns.iter().any(|column| column == "status"));
}
