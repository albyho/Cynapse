use cynapse_client_core::{
    CommandExecutionStart, CommandInterruption, CourseSession, CourseTreeNodeView,
    PlaybackInterruptionReason, UnitLocation, UnitStatus, UnitView,
};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shared = manifest.join("../../course/test-data").join(name);
    if shared.is_dir() {
        shared
    } else {
        manifest.join("../test-data").join(name)
    }
}

fn presentation_only_course(root: &Path) {
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "presentation_only",
            "title": "Presentation Only",
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
        root.join("steps/step.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();
}

fn unit_by_path<'a>(nodes: &'a [CourseTreeNodeView], path: &str) -> &'a UnitView {
    fn find<'a>(nodes: &'a [CourseTreeNodeView], path: &str) -> Option<&'a UnitView> {
        for node in nodes {
            if let Some(unit) = node.unit.as_ref().filter(|unit| unit.path == path) {
                return Some(unit);
            }
            if let Some(unit) = find(&node.children, path) {
                return Some(unit);
            }
        }
        None
    }

    find(nodes, path).unwrap_or_else(|| panic!("unit {path} not found"))
}

fn copy_fixture(name: &str, destination: &Path) {
    fn copy(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    copy(&fixture(name), destination);
}

fn single_step_course(root: &Path) {
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "single_step_course",
            "title": "Single Step Course",
            "adapters": ["vscode"],
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
        root.join("steps/step.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{
                "type": "CreateFile",
                "path": "done.txt"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
}

fn duplicate_unit_ids_course(root: &Path) {
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "duplicate_unit_ids",
            "title": "Duplicate Unit IDs",
            "adapters": ["vscode"],
            "children": [
                {
                    "id": "node_one",
                    "title": "Grouping One",
                    "children": [{
                        "id": "shared",
                        "title": "Shared One",
                        "steps": [{"step_file": "steps/step_one.json"}]
                    }]
                },
                {
                    "id": "node_two",
                    "title": "Grouping Two",
                    "children": [{
                        "id": "shared",
                        "title": "Shared Two",
                        "steps": [{"step_file": "steps/step_two.json"}]
                    }]
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    for step_id in ["step_one", "step_two"] {
        fs::write(
            root.join("steps").join(format!("{step_id}.json")),
            serde_json::to_vec(&serde_json::json!({
                "step_id": step_id,
                "title": step_id,
                "actions": [{
                    "type": "CreateFile",
                    "path": format!("{step_id}.txt")
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn units_share_course_progress_and_keep_independent_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let package = directory.path().join("course");
    copy_fixture("independent-units", &package);
    session.open_course(&package).unwrap();

    session
        .start_unit(&format!("{}/{}", "ch", "unit_c"), true)
        .unwrap();
    session
        .record_progress("ch/unit_c", "shared_1_node_unit_c", true)
        .unwrap();

    let advanced = session.state();
    assert!(
        advanced.selected_unit.is_none(),
        "the next Unit must not be selected before explicit navigation"
    );
    let advanced_course = advanced.active_course.as_ref().unwrap();
    let next_unit = unit_by_path(&advanced_course.tree, "ch/unit_d");
    assert_eq!(next_unit.status, UnitStatus::NotStarted);

    let started_d = session
        .start_unit(&format!("{}/{}", "ch", "unit_d"), true)
        .unwrap();
    let course = started_d.active_course.unwrap();
    assert!(course.has_progress());
    assert_eq!(course.current_unit_path.as_deref(), Some("ch/unit_d"));
    assert_eq!(
        unit_by_path(&course.tree, "ch/unit_c").status,
        UnitStatus::Completed
    );
    assert_eq!(
        unit_by_path(&course.tree, "ch/unit_d").status,
        UnitStatus::NotStarted,
        "selecting and preparing a Unit must not count as learning progress"
    );

    session
        .record_progress("ch/unit_d", "shared_1_node_unit_d", false)
        .unwrap();
    let started_course = session.state().active_course.unwrap();
    assert_eq!(
        unit_by_path(&started_course.tree, "ch/unit_d").status,
        UnitStatus::InProgress
    );
}

#[test]
fn completing_the_last_unit_clears_the_cursor_even_when_earlier_units_are_unfinished() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let package = directory.path().join("course");
    copy_fixture("independent-units", &package);
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit_e", true).unwrap();

    session
        .record_progress("ch/unit_e", "shared_1_node_unit_e", true)
        .unwrap();

    let state = session.state();
    let course = state.active_course.as_ref().unwrap();
    assert_eq!(course.current_unit_path, None);
    assert_eq!(course.current_step_id, None);
    assert!(state.selected_unit.is_none());
    assert!(course.has_progress());
    assert_eq!(
        unit_by_path(&course.tree, "ch/unit_e").status,
        UnitStatus::Completed
    );
    assert_eq!(
        unit_by_path(&course.tree, "ch/unit_a").status,
        UnitStatus::NotStarted
    );

    let reopened = CourseSession::new(directory.path().join("state.db")).unwrap();
    let reopened_state = reopened.state();
    assert_eq!(
        reopened_state
            .active_course
            .as_ref()
            .and_then(|course| course.current_unit_path.clone()),
        None
    );
}

#[test]
fn completed_units_can_reset_and_start_again() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let package = directory.path().join("course");
    copy_fixture("multi-unit-demo", &package);
    session.open_course(&package).unwrap();

    session
        .start_unit(&format!("{}/{}", "ch_units", "first_unit"), true)
        .unwrap();
    session
        .record_progress("ch_units/first_unit", "shared_1", true)
        .unwrap();
    session
        .start_unit(&format!("{}/{}", "ch_units", "second_unit"), true)
        .unwrap();
    session
        .record_progress(
            "ch_units/second_unit",
            "shared_1_ch_units_second_unit",
            true,
        )
        .unwrap();
    let course = session.state().active_course.unwrap();
    assert_eq!(
        unit_by_path(&course.tree, "ch_units/first_unit").status,
        UnitStatus::Completed
    );
    assert_eq!(
        unit_by_path(&course.tree, "ch_units/second_unit").status,
        UnitStatus::Completed
    );

    let reset = session
        .reset_unit(&format!("{}/{}", "ch_units", "first_unit"), true)
        .unwrap();
    let reset_course = reset.state.active_course.unwrap();
    assert_eq!(
        unit_by_path(&reset_course.tree, "ch_units/first_unit").status,
        UnitStatus::NotStarted
    );

    session
        .start_unit(&format!("{}/{}", "ch_units", "first_unit"), true)
        .unwrap();
}

#[test]
fn starting_an_already_completed_unit_requires_reset_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let package = directory.path().join("course");
    copy_fixture("multi-unit-demo", &package);
    session.open_course(&package).unwrap();

    session.start_unit("ch_units/first_unit", true).unwrap();
    session
        .record_progress("ch_units/first_unit", "shared_1", true)
        .unwrap();

    let error = session
        .start_unit("ch_units/first_unit", false)
        .unwrap_err();
    assert!(matches!(
        error,
        cynapse_client_core::CourseSessionError::ConfirmationRequired
    ));
}

#[test]
fn continuous_transition_marks_a_learned_later_unit_for_reset() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let package = directory.path().join("course");
    copy_fixture("multi-unit-demo", &package);
    session.open_course(&package).unwrap();

    session
        .start_unit(&format!("{}/{}", "ch_units", "first_unit"), true)
        .unwrap();
    session
        .record_progress("ch_units/first_unit", "shared_1", true)
        .unwrap();
    session
        .start_unit(&format!("{}/{}", "ch_units", "second_unit"), false)
        .unwrap();
    session
        .record_progress(
            "ch_units/second_unit",
            "shared_1_ch_units_second_unit",
            true,
        )
        .unwrap();

    let transition = session
        .continuous_transition_after("ch_units/first_unit")
        .unwrap()
        .unwrap();
    assert_eq!(
        transition.location,
        UnitLocation::new("ch_units/second_unit")
    );
    assert!(transition.reset_required);
    assert_eq!(
        transition.reset_workspace_paths,
        vec!["ch_units/second_unit".to_string()]
    );
}

#[test]
fn continuous_transition_does_not_start_the_next_unit_before_validation() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let package = directory.path().join("course");
    copy_fixture("multi-unit-demo", &package);
    session.open_course(&package).unwrap();

    session
        .start_unit(&format!("{}/{}", "ch_units", "first_unit"), true)
        .unwrap();
    session
        .record_progress("ch_units/first_unit", "shared_1", true)
        .unwrap();
    let next_workspace = package.join("workspace/ch_units/second_unit");
    assert!(!next_workspace.exists());

    let transition = session
        .continuous_transition_after("ch_units/first_unit")
        .unwrap()
        .unwrap();
    assert_eq!(
        transition.location,
        UnitLocation::new("ch_units/second_unit")
    );
    assert!(!next_workspace.exists());

    session.start_continuous_transition(&transition).unwrap();
    assert!(next_workspace.exists());
}

#[test]
fn clearing_selection_preserves_completed_and_unstarted_unit_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let package = directory.path().join("course");
    copy_fixture("independent-units", &package);
    session.open_course(&package).unwrap();
    session
        .start_unit(&format!("{}/{}", "ch", "unit_c"), true)
        .unwrap();

    session
        .record_progress("ch/unit_c", "shared_1_node_unit_c", true)
        .unwrap();
    let state = session.clear_selection().unwrap();
    let course = state.active_course.as_ref().unwrap();

    assert_eq!(course.current_unit_path, None);
    assert_eq!(course.current_step_id, None);
    assert!(state.selected_unit.is_none());
    let course = state.active_course.unwrap();
    assert_eq!(
        unit_by_path(&course.tree, "ch/unit_c").status,
        UnitStatus::Completed
    );
    assert_eq!(
        unit_by_path(&course.tree, "ch/unit_d").status,
        UnitStatus::NotStarted
    );
    let connection = Connection::open(directory.path().join("state.db")).unwrap();
    let persisted_cursor: Option<String> = connection
        .query_row("SELECT current_unit_path FROM course_progress", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(persisted_cursor, None);

    let reopened = CourseSession::new(directory.path().join("state.db")).unwrap();
    let reopened_state = reopened.state();
    assert!(
        reopened_state.selected_unit.is_none(),
        "an explicit deselection survives a restart"
    );
    assert_eq!(
        reopened_state
            .active_course
            .as_ref()
            .and_then(|course| course.current_unit_path.clone()),
        None
    );
    assert_eq!(
        unit_by_path(&reopened_state.active_course.unwrap().tree, "ch/unit_c").status,
        UnitStatus::Completed
    );
}

#[test]
fn completed_units_can_restart_and_reset() {
    let package = tempfile::tempdir().unwrap();
    single_step_course(package.path());
    let state_directory = tempfile::tempdir().unwrap();
    let db_path = state_directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(package.path()).unwrap();
    session
        .start_unit(&format!("{}/{}", "ch", "unit"), true)
        .unwrap();
    session.record_progress("ch/unit", "step", true).unwrap();
    let completed = session.state();
    let completed_course = completed.active_course.as_ref().unwrap();
    assert!(completed_course.has_progress());
    assert_eq!(completed_course.current_unit_path, None);
    assert_eq!(completed_course.current_step_id, None);

    let restarted = session
        .start_unit(&format!("{}/{}", "ch", "unit"), true)
        .unwrap();
    let restarted_course = restarted.active_course.as_ref().unwrap();
    assert_eq!(
        restarted_course.current_unit_path.as_deref(),
        Some("ch/unit")
    );

    session.record_progress("ch/unit", "step", true).unwrap();
    let reset = session.reset_course(true).unwrap();
    let reset_course = reset.active_course.as_ref().unwrap();
    assert!(!reset_course.has_progress());
    assert_eq!(
        reset
            .active_course
            .map(|course| unit_by_path(&course.tree, "ch/unit").status),
        Some(UnitStatus::NotStarted)
    );
}

#[test]
fn matching_unit_ids_in_different_subtrees_remain_distinct() {
    let package = tempfile::tempdir().unwrap();
    duplicate_unit_ids_course(package.path());
    let state_directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(state_directory.path().join("state.db")).unwrap();
    session.open_course(package.path()).unwrap();

    let state = session
        .start_unit(&format!("{}/{}", "node_one", "shared"), true)
        .unwrap();
    assert_eq!(
        state.selected_unit.as_ref().map(|unit| unit.path.as_str()),
        Some("node_one/shared")
    );
}

#[test]
fn resetting_a_matching_unit_in_another_subtree_keeps_the_current_position() {
    let package = tempfile::tempdir().unwrap();
    duplicate_unit_ids_course(package.path());
    let state_directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(state_directory.path().join("state.db")).unwrap();
    session.open_course(package.path()).unwrap();
    session
        .start_unit(&format!("{}/{}", "node_one", "shared"), true)
        .unwrap();
    session
        .record_progress("node_one/shared", "step_one", false)
        .unwrap();
    session
        .start_unit(&format!("{}/{}", "node_two", "shared"), true)
        .unwrap();

    let state = session
        .reset_unit(&format!("{}/{}", "node_one", "shared"), true)
        .unwrap()
        .state;
    let course = state.active_course.unwrap();

    assert_eq!(course.current_unit_path.as_deref(), Some("node_two/shared"));
    assert_eq!(course.current_step_id.as_deref(), Some("step_two"));
}

#[test]
fn pausing_one_unit_does_not_mark_a_newly_selected_unit_paused() {
    let state_directory = tempfile::tempdir().unwrap();
    let package = state_directory.path().join("course");
    copy_fixture("terminal-recovery-demo", &package);
    let mut session = CourseSession::new(state_directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session
        .start_unit(&format!("{}/{}", "ch_long_running", "main"), true)
        .unwrap();
    session
        .record_progress("ch_long_running/main", "start_long_command_2", false)
        .unwrap();
    session
        .record_command_started(CommandExecutionStart {
            unit_path: "ch_long_running/main",
            step_id: "start_long_command_2",
            action_index: 0,
            command: "Start-Sleep -Seconds 30",
            cwd: ".",
            timeout_ms: 30_000,
        })
        .unwrap();
    session
        .record_command_interrupted(CommandInterruption {
            unit_path: "ch_long_running/main".to_string(),
            step_id: "start_long_command_2".to_string(),
            action_index: 0,
            reason: PlaybackInterruptionReason::UserStop,
            exit_code: None,
            output: None,
            output_truncated: false,
        })
        .unwrap();

    let state = session
        .start_unit(&format!("{}/{}", "ch_timeout", "main"), true)
        .unwrap();
    let course = state.active_course.unwrap();

    assert_eq!(
        unit_by_path(&course.tree, "ch_long_running/main").status,
        UnitStatus::Paused,
        "the inactive Unit keeps its paused state"
    );
    assert_eq!(
        unit_by_path(&course.tree, "ch_timeout/main").status,
        UnitStatus::NotStarted
    );
}

#[test]
fn selecting_a_terminal_unit_does_not_create_progress() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_fixture("terminal-recovery-demo", &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    let state = session
        .select_unit(&format!("{}/{}", "ch_long_running", "main"))
        .unwrap();
    assert_eq!(
        state.selected_unit.as_ref().map(|unit| unit.path.as_str()),
        Some("ch_long_running/main")
    );
    let course = state.active_course.unwrap();
    assert_eq!(
        unit_by_path(&course.tree, "ch_long_running/main").status,
        UnitStatus::NotStarted
    );
    let connection = Connection::open(directory.path().join("state.db")).unwrap();
    let executions: i64 = connection
        .query_row("SELECT COUNT(*) FROM command_executions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(executions, 0);
}

#[test]
fn selecting_an_unstarted_unit_drops_the_previous_units_command_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_fixture("independent-units", &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session
        .start_unit(&format!("{}/{}", "ch", "unit_c"), true)
        .unwrap();
    session
        .record_command_started(CommandExecutionStart {
            unit_path: "ch/unit_c",
            step_id: "shared_1_node_unit_c",
            action_index: 0,
            command: "failed-command",
            cwd: ".",
            timeout_ms: 30_000,
        })
        .unwrap();
    session
        .record_command_interrupted(CommandInterruption {
            unit_path: "ch/unit_c".to_string(),
            step_id: "shared_1_node_unit_c".to_string(),
            action_index: 0,
            reason: PlaybackInterruptionReason::CommandFailure,
            exit_code: Some(1),
            output: Some("failed".to_string()),
            output_truncated: false,
        })
        .unwrap();

    session
        .select_unit(&format!("{}/{}", "ch", "unit_d"))
        .unwrap();
    let resume = session.playback_resume().unwrap();

    assert_eq!(resume.unit_path, "ch/unit_d");
    assert!(resume.recovery.is_none());
}

#[test]
fn starting_a_terminal_unit_authorizes_commands_without_a_separate_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    copy_fixture("rust-book-zh-cn", &package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    let state = session
        .start_unit(&format!("{}/{}", "ch01_getting_started", "main"), false)
        .unwrap();

    assert!(state.selected_unit.is_some());
}

#[test]
fn command_execution_audit_records_cwd_and_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    let package = directory.path().join("course");
    copy_fixture("terminal-recovery-demo", &package);
    session.open_course(&package).unwrap();
    session
        .start_unit(&format!("{}/{}", "ch_long_running", "main"), false)
        .unwrap();
    session
        .record_command_started(CommandExecutionStart {
            unit_path: "ch_long_running/main",
            step_id: "start_long_command_2",
            action_index: 0,
            command: "Start-Sleep -Seconds 30",
            cwd: "ch/unit",
            timeout_ms: 30_000,
        })
        .unwrap();

    let connection = Connection::open(&db_path).unwrap();
    let (cwd, timeout_ms): (String, i64) = connection
        .query_row(
            "SELECT cwd, timeout_ms FROM command_executions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(cwd, "ch/unit");
    assert_eq!(timeout_ms, 30_000);
}

#[test]
fn external_reader_sees_course_progress_while_the_session_is_open() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let package = directory.path().join("course");
    copy_fixture("multi-step-demo", &package);
    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch_multi_step/main", true).unwrap();
    session
        .record_progress("ch_multi_step/main", "write_first", true)
        .unwrap();

    let connection = Connection::open(&db_path).unwrap();
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    let (current_unit, current_step): (Option<String>, Option<String>) = connection
        .query_row(
            "SELECT current_unit_path, current_step_id FROM course_progress",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let completed: i64 = connection
        .query_row(
            "SELECT completed FROM step_states
             WHERE unit_path = 'ch_multi_step/main' AND step_id = 'write_first'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(journal_mode, "wal");
    assert_eq!(current_unit.as_deref(), Some("ch_multi_step/main"));
    assert_eq!(current_step.as_deref(), Some("write_second"));
    assert_eq!(completed, 1);
}

#[test]
fn command_reconciliation_skips_the_update_when_no_command_is_running() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let package = directory.path().join("course");
    copy_fixture("multi-step-demo", &package);
    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch_multi_step/main", true).unwrap();

    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE command_updates (id INTEGER PRIMARY KEY AUTOINCREMENT);
            CREATE TRIGGER count_command_update
            AFTER UPDATE ON command_executions
            BEGIN
                INSERT INTO command_updates(id) VALUES (NULL);
            END;
            INSERT INTO command_executions(
                course_id, unit_path, step_id, action_index,
                command, cwd, timeout_ms, status, output, output_truncated,
                exit_code, interruption, started_at, updated_at
            )
            VALUES (
                'multi_step_demo', 'ch_multi_step/main', 'write_first', 0,
                'node --test', '.', 1000, 'running', NULL, 0,
                NULL, NULL, 1, 1
            );
            ",
        )
        .unwrap();

    session
        .complete_running_commands_except("ch_multi_step/main", "write_first", 1)
        .unwrap();

    let updates: i64 = connection
        .query_row("SELECT COUNT(*) FROM command_updates", [], |row| row.get(0))
        .unwrap();
    let status: String = connection
        .query_row(
            "SELECT status FROM command_executions
             WHERE unit_path = 'ch_multi_step/main' AND action_index = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(updates, 0);
    assert_eq!(status, "running");
}

#[test]
fn legacy_command_authorization_state_is_rebuilt_with_the_current_schema() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE command_confirmations (
                id TEXT NOT NULL,
                version TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                confirmed_at INTEGER NOT NULL
            );
            PRAGMA user_version = 2;
            ",
        )
        .unwrap();
    drop(connection);

    CourseSession::new(&db_path).unwrap();

    let connection = Connection::open(&db_path).unwrap();
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let authorization_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'command_confirmations'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(user_version, 12);
    assert_eq!(authorization_tables, 0);
}

#[test]
fn missing_workspace_uses_the_default_course_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    single_step_course(&package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    session.open_course(&package).unwrap();
    let expected_workspace = package.join("workspace").canonicalize().unwrap();
    assert_eq!(
        session
            .active_workspace()
            .map(|workspace| workspace.canonicalize().unwrap()),
        Some(expected_workspace)
    );

    session.start_unit("ch/unit", true).unwrap();

    assert!(package.join("workspace/ch/unit").is_dir());
}

#[test]
fn presentation_only_unit_does_not_create_a_unit_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("course");
    presentation_only_course(&package);
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();

    session.open_course(&package).unwrap();
    assert!(package.join("workspace").is_dir());

    session.start_unit("ch/unit", true).unwrap();

    assert!(!package.join("workspace/ch/unit").exists());
}
