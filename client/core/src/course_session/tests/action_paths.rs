//! Unit-relative Action path policy.

use super::super::{PathPolicy, prepare_unit_action, unit_action_path};
use crate::{
    Action, CloseFileAction, ClosePreviewAction, CreateFileAction, DeleteDirectoryAction,
    InsertTextTarget, JupyterLabCellInsertTextAction, JupyterLabCellRunAction,
    JupyterLabCellSelectAction, JupyterLabInterruptKernelAction, JupyterLabOpenNotebookAction,
    JupyterLabRestartKernelAction, MissingTargetPolicy, OpenFileAction, TerminalCommandAction,
    load_package,
};
use std::path::Path;

fn package() -> (crate::Course, std::path::PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-data/independent-units");
    let course = load_package(&root).unwrap();
    (course, root.join("workspace/resources"))
}

fn terminal(cwd: &str, marker: Option<&str>) -> Action {
    Action::TerminalCommand(TerminalCommandAction {
        session_key: "main".to_string(),
        cwd: cwd.to_string(),
        command: "echo hi".to_string(),
        accepted_exit_codes: vec![0],
        expected_output: Vec::new(),
        timeout_ms: 1000,
        completion_marker: marker.map(str::to_string),
        new_terminal: false,
    })
}

#[test]
fn write_paths_reject_any_parent_directory_segment() {
    for path in ["../advanced", "src/../main.rs", "../../resources/x"] {
        let error = unit_action_path("ch/intro", path, PathPolicy::UnitWorkspaceOnly)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("must stay inside the Unit Workspace"),
            "{path}: {error}"
        );
    }
}

#[test]
fn write_paths_resolve_inside_the_unit() {
    assert_eq!(
        unit_action_path("ch/intro", "src/main.rs", PathPolicy::UnitWorkspaceOnly).unwrap(),
        "ch/intro/src/main.rs"
    );
}

#[test]
fn read_paths_may_reach_shared_resources() {
    assert_eq!(
        unit_action_path(
            "ch/intro",
            "../../resources/agent-desktop/README.md",
            PathPolicy::ReadOnly,
        )
        .unwrap(),
        "resources/agent-desktop/README.md"
    );
}

#[test]
fn read_paths_may_reach_a_sibling_unit_inside_the_course_workspace() {
    assert_eq!(
        unit_action_path("ch/intro", "../advanced/secret.txt", PathPolicy::ReadOnly).unwrap(),
        "ch/advanced/secret.txt"
    );
}

#[test]
fn read_paths_still_cannot_leave_the_course_workspace() {
    let error = unit_action_path("ch/intro", "../../../outside", PathPolicy::ReadOnly)
        .unwrap_err()
        .to_string();

    assert!(error.contains("escapes the Course Workspace"), "{error}");
}

#[test]
fn write_actions_and_terminal_locations_stay_inside_the_unit_workspace() {
    let (course, resources) = package();
    let unit_path = course.units.first().unwrap().path.clone();
    let write = Action::CreateFile(CreateFileAction {
        path: "../advanced/main.rs".to_string(),
        create_parents: true,
    });

    let error = prepare_unit_action(&course, &write, &unit_path, &resources)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Unit Workspace"), "{error}");

    for action in [
        terminal("../advanced", None),
        terminal(".", Some("../marker.txt")),
    ] {
        let error = prepare_unit_action(&course, &action, &unit_path, &resources)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Unit Workspace"), "{error}");
    }
}

#[test]
fn delete_directory_rejects_the_unit_workspace_root() {
    let (course, resources) = package();
    let unit_path = course.units.first().unwrap().path.clone();
    for path in ["", ".", "./"] {
        let action = Action::DeleteDirectory(DeleteDirectoryAction {
            path: path.to_string(),
            recursive: true,
            if_missing: MissingTargetPolicy::Error,
        });

        let error = prepare_unit_action(&course, &action, &unit_path, &resources)
            .unwrap_err()
            .to_string();

        assert!(error.contains("DeleteDirectory path"), "{error}");
        assert!(error.contains("Unit Workspace root"), "{error}");
    }
}

#[test]
fn read_actions_may_leave_the_unit_workspace() {
    let (course, resources) = package();
    let unit_path = course.units.first().unwrap().path.clone();
    let read_actions = [
        Action::OpenFile(OpenFileAction {
            path: "../advanced/secret.txt".to_string(),
        }),
        Action::CloseFile(CloseFileAction {
            path: "../advanced/secret.txt".to_string(),
        }),
        Action::ClosePreview(ClosePreviewAction {
            path: "../advanced/secret.txt".to_string(),
        }),
    ];

    for read in read_actions {
        match prepare_unit_action(&course, &read, &unit_path, &resources).unwrap() {
            Action::OpenFile(action) => assert_eq!(action.path, "ch/advanced/secret.txt"),
            Action::CloseFile(action) => assert_eq!(action.path, "ch/advanced/secret.txt"),
            Action::ClosePreview(action) => assert_eq!(action.path, "ch/advanced/secret.txt"),
            other => panic!("expected a read-only action, got {other:?}"),
        }
    }
}

#[test]
fn notebook_actions_resolve_inside_the_unit_workspace() {
    let (course, resources) = package();
    let unit_path = course.units.first().unwrap().path.clone();
    let action = Action::JupyterLabCellSelect(JupyterLabCellSelectAction {
        path: "lesson.ipynb".to_string(),
        cell_id: "cell-1".to_string(),
        scroll: true,
        align: None,
    });

    match prepare_unit_action(&course, &action, &unit_path, &resources).unwrap() {
        Action::JupyterLabCellSelect(action) => assert_eq!(action.path, "ch/unit_a/lesson.ipynb"),
        other => panic!("expected JupyterLabCellSelect, got {other:?}"),
    }
}

#[test]
fn notebook_actions_cannot_leave_the_unit_workspace() {
    let (course, resources) = package();
    let unit_path = course.units.first().unwrap().path.clone();
    let path = "../advanced/lesson.ipynb".to_string();
    let cell_id = "cell-1".to_string();
    let actions = [
        Action::JupyterLabOpenNotebook(JupyterLabOpenNotebookAction { path: path.clone() }),
        Action::JupyterLabCellSelect(JupyterLabCellSelectAction {
            path: path.clone(),
            cell_id: cell_id.clone(),
            scroll: false,
            align: None,
        }),
        Action::JupyterLabCellInsertText(JupyterLabCellInsertTextAction {
            path: path.clone(),
            cell_id: cell_id.clone(),
            target: InsertTextTarget {
                line: None,
                at: None,
                placement: None,
                eof: true,
            },
            text: "print(1)".to_string(),
            speed_ms: None,
            target_hold_ms: None,
        }),
        Action::JupyterLabCellRun(JupyterLabCellRunAction {
            path: path.clone(),
            from_cell_id: Some(cell_id.clone()),
            to_cell_id: Some(cell_id.clone()),
            timeout_ms: 1000,
        }),
        Action::JupyterLabRestartKernel(JupyterLabRestartKernelAction { path: path.clone() }),
        Action::JupyterLabInterruptKernel(JupyterLabInterruptKernelAction { path }),
    ];

    for action in actions {
        let error = prepare_unit_action(&course, &action, &unit_path, &resources)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Unit Workspace"), "{action:?}: {error}");
    }
}
