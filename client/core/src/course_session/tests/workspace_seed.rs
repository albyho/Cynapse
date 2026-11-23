use super::super::*;
use super::support::{copy_directory, fixture_path};
use crate::course::Action;

/// 每个 Unit 的 Seed 只来自 `seeds/<unit path>/`。这里刻意让
/// `seeds/ch/unit/` 里嵌套后代 `seeds/ch/unit/child/`，并把
/// `seeds/ch/container/` 做成「只承载后代 Seed」的路径容器。
fn scoped_seed_course(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("steps")).unwrap();
    std::fs::create_dir_all(root.join("workspace")).unwrap();
    std::fs::create_dir_all(root.join("resources/include")).unwrap();
    for directory in [
        "seeds/ch/unit/child",
        "seeds/ch/unit",
        "seeds/ch/other",
        "seeds/ch/container/leaf",
    ] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "scoped_seed",
            "title": "Scoped Seed",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": [{"step_file": "steps/step.json"}],
                    "children": [{
                        "id": "child",
                        "title": "Child Unit",
                        "steps": [{"step_file": "steps/present.json"}]
                    }]
                }, {
                    "id": "other",
                    "title": "Other Unit",
                    "steps": [{"step_file": "steps/step.json"}]
                }, {
                    "id": "container",
                    "title": "Container Unit",
                    "steps": [{"step_file": "steps/present.json"}],
                    "children": [{
                        "id": "leaf",
                        "title": "Leaf Unit",
                        "steps": [{"step_file": "steps/present.json"}]
                    }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("steps/step.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [
                {"type": "CreateFile", "path": "result.txt"},
                {
                    "type": "TerminalCommand",
                    "session_key": "main",
                    "cwd": ".",
                    "command": "tool -I \"{{resource:course}}/include\"",
                    "completion_marker": "marker.txt"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("steps/present.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "present",
            "title": "Present",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("seeds/ch/unit/base.txt"), "unit base\n").unwrap();
    std::fs::write(root.join("seeds/ch/unit/override.txt"), "unit\n").unwrap();
    std::fs::write(root.join("seeds/ch/unit/unit.txt"), "unit\n").unwrap();
    std::fs::write(root.join("seeds/ch/unit/child/child.txt"), "child\n").unwrap();
    std::fs::write(root.join("seeds/ch/other/other.txt"), "other\n").unwrap();
    std::fs::write(root.join("seeds/ch/container/leaf/leaf.txt"), "leaf\n").unwrap();
    root.to_path_buf()
}

fn seedless_course(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("steps")).unwrap();
    std::fs::create_dir_all(root.join("workspace")).unwrap();
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "seedless",
            "title": "Seedless",
            "workspace": "workspace",
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
    std::fs::write(
        root.join("steps/step.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{
                "type": "Present"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    root.to_path_buf()
}

fn root_unit_course(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("steps")).unwrap();
    std::fs::create_dir_all(root.join("workspace")).unwrap();
    std::fs::create_dir_all(root.join("seeds/solo")).unwrap();
    std::fs::write(root.join("seeds/solo/seed.txt"), "seed\n").unwrap();
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "root_unit",
            "title": "Root Unit",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "solo",
                "title": "Solo",
                "steps": [{"step_file": "steps/step.json"}]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("steps/step.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{
                "type": "CreateFile",
                "path": "created.txt"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    root.to_path_buf()
}

#[test]
fn resetting_a_root_unit_keeps_the_declared_workspace_root() {
    let state = tempfile::tempdir().unwrap();
    let package = root_unit_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("solo", true).unwrap();
    session.record_progress("solo", "step", false).unwrap();

    session.reset_unit("solo", true).unwrap();

    assert!(package.join("workspace").is_dir());
    assert!(!package.join("workspace/solo").exists());

    session.start_unit("solo", true).unwrap();

    assert_eq!(
        std::fs::read_to_string(package.join("workspace/solo/seed.txt")).unwrap(),
        "seed\n"
    );
}

#[test]
fn starting_a_reset_unit_replaces_a_recreated_stale_workspace() {
    let state = tempfile::tempdir().unwrap();
    let package = root_unit_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("solo", true).unwrap();
    session.record_progress("solo", "step", false).unwrap();
    session.reset_unit("solo", true).unwrap();
    std::fs::create_dir_all(package.join("workspace/solo")).unwrap();
    std::fs::write(package.join("workspace/solo/seed.txt"), "stale\n").unwrap();

    session.start_unit("solo", true).unwrap();

    assert_eq!(
        std::fs::read_to_string(package.join("workspace/solo/seed.txt")).unwrap(),
        "seed\n"
    );
}

fn assert_linked_workspace_is_rejected(state: &Path, package: &Path, outside: &Path) {
    let victim = outside.join("victim.txt");
    std::fs::write(&victim, "do not delete").unwrap();

    let mut session = CourseSession::new(state.join("state.db")).unwrap();
    session.open_course(package).unwrap();

    let error = session.start_unit("solo", true).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link or junction"),
        "{error}"
    );
    assert!(victim.exists(), "the outside file must be untouched");
}

// Junction is Windows-specific. POSIX covers the same containment rule with a
// directory symlink in the test below.
#[cfg(windows)]
#[test]
fn starting_a_unit_rejects_a_junction_workspace() {
    let state = tempfile::tempdir().unwrap();
    let package = root_unit_course(&state.path().join("course"));
    let outside = tempfile::tempdir().unwrap();
    super::support::make_junction(&package.join("workspace").join("solo"), outside.path());

    assert_linked_workspace_is_rejected(state.path(), &package, outside.path());
}

// POSIX has no junction; a directory symlink exercises the same guard.
#[cfg(unix)]
#[test]
fn starting_a_unit_rejects_a_symlink_workspace() {
    let state = tempfile::tempdir().unwrap();
    let package = root_unit_course(&state.path().join("course"));
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), package.join("workspace").join("solo")).unwrap();

    assert_linked_workspace_is_rejected(state.path(), &package, outside.path());
}

fn ancestor_link_course(state: &Path) -> (PathBuf, tempfile::TempDir) {
    let package = scoped_seed_course(&state.join("course"));
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, "do not delete").unwrap();
    (package, outside)
}

#[cfg(windows)]
#[test]
fn starting_a_unit_rejects_a_junction_ancestor() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = ancestor_link_course(state.path());
    super::support::make_junction(&package.join("workspace").join("ch"), outside.path());

    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    let error = session.start_unit("ch/unit", true).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link or junction"),
        "{error}"
    );
    assert!(outside.path().join("victim.txt").exists());
}

#[cfg(unix)]
#[test]
fn starting_a_unit_rejects_a_symlink_ancestor() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = ancestor_link_course(state.path());
    std::os::unix::fs::symlink(outside.path(), package.join("workspace/ch")).unwrap();

    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    let error = session.start_unit("ch/unit", true).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link or junction"),
        "{error}"
    );
    assert!(outside.path().join("victim.txt").exists());
}

fn prepare_ancestor_link_reset(state: &Path) -> (PathBuf, tempfile::TempDir) {
    let (package, outside) = ancestor_link_course(state);
    std::fs::create_dir_all(outside.path().join("unit")).unwrap();
    std::fs::write(outside.path().join("unit/keep.txt"), "do not delete").unwrap();
    (package, outside)
}

#[cfg(windows)]
#[test]
fn resetting_a_unit_rejects_a_junction_ancestor() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = prepare_ancestor_link_reset(state.path());
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", false).unwrap();
    std::fs::remove_dir_all(package.join("workspace").join("ch")).unwrap();
    super::support::make_junction(&package.join("workspace").join("ch"), outside.path());

    let error = session.reset_unit("ch/unit", true).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link or junction"),
        "{error}"
    );
    assert!(outside.path().join("unit/keep.txt").exists());
}

#[cfg(unix)]
#[test]
fn resetting_a_unit_rejects_a_symlink_ancestor() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = prepare_ancestor_link_reset(state.path());
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", false).unwrap();
    std::fs::remove_dir_all(package.join("workspace/ch")).unwrap();
    std::os::unix::fs::symlink(outside.path(), package.join("workspace/ch")).unwrap();

    let error = session.reset_unit("ch/unit", true).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link or junction"),
        "{error}"
    );
    assert!(outside.path().join("unit/keep.txt").exists());
}

#[cfg(windows)]
#[test]
fn starting_a_unit_rejects_a_junction_workspace_root() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = ancestor_link_course(state.path());
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    std::fs::remove_dir_all(package.join("workspace")).unwrap();
    super::support::make_junction(&package.join("workspace"), outside.path());

    let error = session.start_unit("ch/unit", true).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link or junction"),
        "{error}"
    );
    assert!(outside.path().join("victim.txt").exists());
}

#[cfg(unix)]
#[test]
fn starting_a_unit_rejects_a_symlink_workspace_root() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = ancestor_link_course(state.path());
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    std::fs::remove_dir_all(package.join("workspace")).unwrap();
    std::os::unix::fs::symlink(outside.path(), package.join("workspace")).unwrap();

    let error = session.start_unit("ch/unit", true).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link or junction"),
        "{error}"
    );
    assert!(outside.path().join("victim.txt").exists());
}

#[cfg(windows)]
#[test]
fn resetting_the_course_rejects_a_junction_workspace_root() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = ancestor_link_course(state.path());
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    std::fs::remove_dir_all(package.join("workspace")).unwrap();
    super::support::make_junction(&package.join("workspace"), outside.path());

    let error = session.reset_course(true).unwrap_err();

    assert!(error.to_string().contains("workspace"), "{error}");
    assert!(outside.path().join("victim.txt").exists());
}

#[cfg(unix)]
#[test]
fn resetting_the_course_rejects_a_symlink_workspace_root() {
    let state = tempfile::tempdir().unwrap();
    let (package, outside) = ancestor_link_course(state.path());
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    std::fs::remove_dir_all(package.join("workspace")).unwrap();
    std::os::unix::fs::symlink(outside.path(), package.join("workspace")).unwrap();

    let error = session.reset_course(true).unwrap_err();

    assert!(error.to_string().contains("workspace"), "{error}");
    assert!(outside.path().join("victim.txt").exists());
}

#[test]
fn resetting_when_the_workspace_root_is_missing_does_not_fail() {
    let state = tempfile::tempdir().unwrap();
    let package = root_unit_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("solo", true).unwrap();
    session.record_progress("solo", "step", false).unwrap();
    std::fs::remove_dir_all(package.join("workspace")).unwrap();

    session.reset_unit("solo", true).unwrap();
}

#[test]
fn resetting_a_unit_whose_workspace_is_missing_does_not_fail() {
    let state = tempfile::tempdir().unwrap();
    let package = root_unit_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("solo", true).unwrap();
    session.record_progress("solo", "step", false).unwrap();
    std::fs::remove_dir_all(package.join("workspace/solo")).unwrap();

    let outcome = session.reset_unit("solo", true).unwrap();

    assert_eq!(
        find_unit(outcome.state.active_course.unwrap().tree.as_slice(), "solo")
            .unwrap()
            .status,
        UnitStatus::NotStarted
    );
}

#[test]
fn resetting_seedless_unit_removes_empty_workspace_directories() {
    let state = tempfile::tempdir().unwrap();
    let package = seedless_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", false).unwrap();
    let unit = package.join("workspace/ch/unit");
    std::fs::create_dir_all(unit.join("empty/nested")).unwrap();
    std::fs::write(unit.join("user-created.txt"), "user data\n").unwrap();

    session.reset_unit("ch/unit", true).unwrap();

    assert!(!unit.exists());
    assert!(!package.join("workspace/ch").exists());
    assert!(package.join("workspace").is_dir());
}

#[test]
fn selecting_a_unit_defers_seed_copy_until_start() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    let workspace = package.join("workspace/ch/unit");

    session.select_unit("ch/unit").unwrap();

    assert!(!workspace.exists());

    session.start_unit("ch/unit", true).unwrap();

    assert_eq!(
        std::fs::read_to_string(workspace.join("base.txt")).unwrap(),
        "unit base\n"
    );
}

#[test]
fn starting_a_unit_seeds_an_empty_workspace_and_rebuilds_it_after_deletion() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    let workspace = package.join("workspace/ch/unit");

    // “初始状态没有 Seed”：只选中 Unit 时目录不存在，第一次开始才落 Seed。
    session.select_unit("ch/unit").unwrap();
    assert!(!workspace.exists());
    session.start_unit("ch/unit", true).unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.join("base.txt")).unwrap(),
        "unit base\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("override.txt")).unwrap(),
        "unit\n"
    );

    // Unit 目录被整个删除（等价于“彻底重置”后的状态）后，
    // 再次开始仍能独立把本 Unit 的 Seed 补齐，不依赖 reset 预先重建。
    std::fs::remove_dir_all(&workspace).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.join("base.txt")).unwrap(),
        "unit base\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("unit.txt")).unwrap(),
        "unit\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("override.txt")).unwrap(),
        "unit\n"
    );
    // 嵌套在父 Seed 目录下的后代 Seed 整棵跳过。
    assert!(!workspace.join("child").exists());
    assert!(!workspace.join("child/child.txt").exists());
}

#[test]
fn starting_a_unit_tops_up_a_deleted_seed_file_without_overwriting_user_edits() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    let workspace = package.join("workspace/ch/unit");

    // 学员删掉了某个 Seed 文件（例如 INTRO.md），同时改过另一个 Seed 文件。
    std::fs::remove_file(workspace.join("base.txt")).unwrap();
    std::fs::write(workspace.join("override.txt"), "my notes\n").unwrap();

    session.start_unit("ch/unit", true).unwrap();

    assert_eq!(
        std::fs::read_to_string(workspace.join("base.txt")).unwrap(),
        "unit base\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("override.txt")).unwrap(),
        "my notes\n"
    );
    // 补缺也不会把后代 Unit 的 Seed 带进来。
    assert!(!workspace.join("child/child.txt").exists());
}

#[test]
fn starting_a_unit_after_a_workspace_only_reset_restores_the_seed() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", false).unwrap();
    let workspace = package.join("workspace/ch/unit");

    session.reset_unit("ch/unit", true).unwrap();
    // 重置后工作区保持“尚未开始”的空状态，此时还没有 Seed。
    assert!(!workspace.exists());

    session.start_unit("ch/unit", true).unwrap();

    assert_eq!(
        std::fs::read_to_string(workspace.join("base.txt")).unwrap(),
        "unit base\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("override.txt")).unwrap(),
        "unit\n"
    );
}

#[test]
fn restarting_a_presentation_only_unit_does_not_create_an_empty_workspace() {
    let state = tempfile::tempdir().unwrap();
    let package = seedless_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    session.start_unit("ch/unit", true).unwrap();
    assert!(!package.join("workspace/ch/unit").exists());

    // 第二次开始时进度仍在进行中（replace=false），同样不该造出空目录。
    session.start_unit("ch/unit", true).unwrap();
    assert!(!package.join("workspace/ch/unit").exists());
    assert!(package.join("workspace").is_dir());
}

#[test]
fn resetting_one_unit_leaves_it_seedless_until_it_starts_again() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", false).unwrap();
    let reset_target = package.join("workspace/ch/unit");
    std::fs::write(reset_target.join("override.txt"), "changed\n").unwrap();
    std::fs::write(reset_target.join("extra.txt"), "extra\n").unwrap();

    session.start_unit("ch/other", true).unwrap();
    let other = package.join("workspace/ch/other");
    std::fs::write(other.join("keep.txt"), "keep\n").unwrap();
    let resource = package.join("resources/include/shared.h");
    std::fs::write(&resource, "shared\n").unwrap();

    session.reset_unit("ch/unit", true).unwrap();

    // 重置只清空这个 Unit 的工作区，Seed 此时还没落下，其他 Unit 与资源不动。
    assert!(!reset_target.exists());
    assert_eq!(
        std::fs::read_to_string(other.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert_eq!(std::fs::read_to_string(&resource).unwrap(), "shared\n");

    // 再次开始（或“继续”）时才从本 Unit 的 Seed 重建，学员的旧改动不会回来。
    session.start_unit("ch/unit", true).unwrap();
    assert_eq!(
        std::fs::read_to_string(reset_target.join("override.txt")).unwrap(),
        "unit\n"
    );
    assert!(!reset_target.join("extra.txt").exists());
    assert_eq!(
        std::fs::read_to_string(other.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert_eq!(std::fs::read_to_string(&resource).unwrap(), "shared\n");
}

#[test]
fn resetting_the_course_clears_nested_workspaces_and_preserves_gitkeep() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", true).unwrap();
    session.start_unit("ch/unit/child", true).unwrap();
    session.start_unit("ch/other", true).unwrap();
    let first = package.join("workspace/ch/unit");
    let child = package.join("workspace/ch/unit/child");
    let second = package.join("workspace/ch/other");
    std::fs::write(first.join("extra.txt"), "extra\n").unwrap();
    std::fs::write(child.join("extra.txt"), "extra\n").unwrap();
    std::fs::write(second.join("extra.txt"), "extra\n").unwrap();
    std::fs::write(package.join("workspace/.gitkeep"), "").unwrap();
    let resource = package.join("resources/include/shared.h");
    std::fs::write(&resource, "shared\n").unwrap();
    assert_eq!(
        session
            .workspace_reset_paths_for_course_reset(true)
            .unwrap(),
        vec![".".to_string()]
    );

    session.reset_course(true).unwrap();

    assert!(!first.exists());
    assert!(!child.exists());
    assert!(!second.exists());
    assert!(package.join("workspace").is_dir());
    assert!(package.join("workspace/.gitkeep").is_file());
    assert!(!package.join("workspace/resources").exists());
    assert_eq!(std::fs::read_to_string(&resource).unwrap(), "shared\n");

    // 逐个开始时才重新落下 Seed 与资源；Course Reset 不预建 Unit 目录。
    session.start_unit("ch/unit", true).unwrap();
    session.start_unit("ch/unit/child", true).unwrap();
    session.start_unit("ch/other", true).unwrap();
    assert!(first.join("override.txt").is_file());
    assert!(child.join("child.txt").is_file());
    assert!(second.join("other.txt").is_file());
    assert!(!first.join("extra.txt").exists());
    assert!(!child.join("extra.txt").exists());
    assert!(!second.join("extra.txt").exists());
    assert!(
        package
            .join("workspace/resources/include/shared.h")
            .is_file()
    );
    assert_eq!(std::fs::read_to_string(&resource).unwrap(), "shared\n");
}

#[test]
fn resetting_the_course_preserves_the_course_environment() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    let environment = package.join("workspace/.venv");
    let interpreter = environment.join("Scripts/python.exe");
    std::fs::create_dir_all(interpreter.parent().unwrap()).unwrap();
    std::fs::write(&interpreter, "").unwrap();
    std::fs::write(environment.join("pyvenv.cfg"), "home = python\n").unwrap();

    session.reset_course(true).unwrap();

    assert!(interpreter.is_file());
    assert_eq!(
        std::fs::read_to_string(environment.join("pyvenv.cfg")).unwrap(),
        "home = python\n"
    );
    assert!(!package.join("workspace/ch/unit").exists());
    assert!(!package.join("workspace/resources").exists());
}

#[test]
fn resetting_the_course_refreshes_seeds_for_the_next_start() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", true).unwrap();
    std::fs::write(package.join("seeds/ch/unit/override.txt"), "updated seed\n").unwrap();

    session.reset_course(true).unwrap();

    let workspace = package.join("workspace/ch/unit");
    assert!(!workspace.exists());
    // 快照在重置时已刷新，下一次开始拿到的是课程包里更新过的 Seed。
    session.start_unit("ch/unit", true).unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.join("override.txt")).unwrap(),
        "updated seed\n"
    );
}

#[test]
fn resetting_the_current_unit_clears_the_current_selection() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    session.record_progress("ch/unit", "step", false).unwrap();

    let reset = session.reset_unit("ch/unit", true).unwrap();

    assert!(reset.state.selected_unit.is_none());
    let course = reset.state.active_course.unwrap();
    assert_eq!(course.current_unit_path, None);
    assert_eq!(course.current_step_id, None);
}

#[test]
fn starting_a_unit_builds_an_isolated_workspace_from_its_own_seed() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    session.start_unit("ch/unit", true).unwrap();

    let workspace = package.join("workspace/ch/unit");
    assert_eq!(
        std::fs::read_to_string(workspace.join("base.txt")).unwrap(),
        "unit base\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("unit.txt")).unwrap(),
        "unit\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("override.txt")).unwrap(),
        "unit\n"
    );
    // 不再有祖先层；后代节点的 Seed 也整棵跳过。
    assert!(!workspace.join("ancestor.txt").exists());
    assert!(!workspace.join("child").exists());
    assert!(!workspace.join("resources").exists());

    let step = session.playback_steps().remove(0);
    let resources = session
        .active_workspace()
        .unwrap()
        .join("resources")
        .canonicalize()
        .unwrap();
    match &step.actions[0].action {
        Action::CreateFile(action) => {
            assert_eq!(action.path, "ch/unit/result.txt");
        }
        action => panic!("expected CreateFile, got {action:?}"),
    }
    match &step.actions[1].action {
        Action::TerminalCommand(action) => {
            assert_eq!(action.cwd, "ch/unit");
            let resources = resources.to_string_lossy();
            let resources = resources.strip_prefix(r"\\?\").unwrap_or(&resources);
            assert!(
                action.command.contains(resources),
                "command={}",
                action.command
            );
            assert!(!action.command.contains("{{resource:"));
            assert_eq!(
                action.completion_marker.as_deref(),
                Some("ch/unit/marker.txt")
            );
        }
        action => panic!("expected TerminalCommand, got {action:?}"),
    }
}

#[test]
fn a_descendant_unit_does_not_inherit_its_parent_seed() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    session.start_unit("ch/unit/child", true).unwrap();

    let workspace = package.join("workspace/ch/unit/child");
    assert_eq!(
        std::fs::read_to_string(workspace.join("child.txt")).unwrap(),
        "child\n"
    );
    for inherited in ["base.txt", "unit.txt", "override.txt"] {
        assert!(!workspace.join(inherited).exists(), "leaked {inherited}");
    }
    // 开始子 Unit 不会顺带创建父 Unit 的工作区。
    assert!(!package.join("workspace/ch/unit/base.txt").exists());
}

#[test]
fn a_container_only_seed_does_not_create_a_workspace() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    // seeds/ch/container/ 下只有后代 seeds/ch/container/leaf/，没有本 Unit 的文件。
    session.start_unit("ch/container", true).unwrap();
    assert!(!package.join("workspace/ch/container").exists());

    session.start_unit("ch/container/leaf", true).unwrap();
    // 叶子是 Present-only，且 Seed 只有一个文件：工作区应包含且仅包含这个文件。
    let leaf = package.join("workspace/ch/container/leaf");
    assert_eq!(
        std::fs::read_to_string(leaf.join("leaf.txt")).unwrap(),
        "leaf\n"
    );
    assert!(!leaf.join("child.txt").exists());
}

#[test]
fn a_used_missing_resource_scope_prevents_the_unit_from_starting() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    std::fs::remove_dir_all(package.join("resources")).unwrap();
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    let error = session.start_unit("ch/unit", true).unwrap_err();

    assert!(error.to_string().contains("resource scope `course`"));
}

#[test]
fn resource_aliases_cannot_be_used_as_write_targets() {
    let state = tempfile::tempdir().unwrap();
    let package = scoped_seed_course(&state.path().join("course"));
    let step_path = package.join("steps/step.json");
    let mut step: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&step_path).unwrap()).unwrap();
    step["actions"][0]["path"] = serde_json::json!("{{resource:course}}/result.txt");
    std::fs::write(&step_path, serde_json::to_vec_pretty(&step).unwrap()).unwrap();
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    let error = session.start_unit("ch/unit", true).unwrap_err();

    assert!(
        error.to_string().contains("must not use resource aliases"),
        "{error}"
    );
}

#[test]
fn a_unit_without_its_own_seed_does_not_inherit_course_or_ancestor_seeds() {
    let state = tempfile::tempdir().unwrap();
    let source = fixture_path("resources-and-seeds-demo");
    let package = state.path().join("course");
    copy_directory(&source, &package);
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();

    session.start_unit("ch_advanced/empty_unit", true).unwrap();

    let workspace = package.join("workspace/ch_advanced/empty_unit");
    // 该 Unit 有 CreateFile，所以工作区会建立，但里面没有任何继承来的 Seed 文件。
    assert!(workspace.is_dir());
    for inherited in ["README.txt", "ancestor.txt", "override.txt"] {
        assert!(!workspace.join(inherited).exists(), "leaked {inherited}");
    }
}

#[test]
fn starting_a_course_copies_resources_into_the_workspace() {
    let state = tempfile::tempdir().unwrap();
    let package = seedless_course(&state.path().join("course"));
    std::fs::create_dir_all(package.join("resources/agent-desktop/nested")).unwrap();
    std::fs::write(
        package.join("resources/agent-desktop/readme.txt"),
        "upstream\n",
    )
    .unwrap();
    std::fs::write(
        package.join("resources/agent-desktop/nested/data.txt"),
        "data\n",
    )
    .unwrap();

    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    assert!(!package.join("workspace/resources").exists());

    session.start_unit("ch/unit", true).unwrap();

    let resources = package.join("workspace/resources/agent-desktop");
    assert_eq!(
        std::fs::read_to_string(resources.join("readme.txt")).unwrap(),
        "upstream\n"
    );
    assert_eq!(
        std::fs::read_to_string(resources.join("nested/data.txt")).unwrap(),
        "data\n"
    );

    // Unit Reset leaves the course-level resource copy untouched.
    std::fs::write(resources.join("readme.txt"), "edited by learner\n").unwrap();
    session.record_progress("ch/unit", "step", false).unwrap();
    session.reset_unit("ch/unit", true).unwrap();
    session.start_unit("ch/unit", true).unwrap();
    assert_eq!(
        std::fs::read_to_string(resources.join("readme.txt")).unwrap(),
        "edited by learner\n",
        "Unit Reset must not touch workspace/resources"
    );

    // A course reset removes the copy; the next Start recreates it from the authored tree.
    session.reset_course(true).unwrap();
    assert!(!resources.exists());
    session.start_unit("ch/unit", true).unwrap();
    assert_eq!(
        std::fs::read_to_string(resources.join("readme.txt")).unwrap(),
        "upstream\n"
    );
}

#[test]
fn seed_copy_modes_differ_only_for_existing_files() {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(source.path().join("nested")).unwrap();
    std::fs::write(source.path().join("existing.txt"), "seed").unwrap();
    std::fs::write(source.path().join("added.txt"), "added").unwrap();
    std::fs::write(source.path().join("nested/existing.txt"), "seed nested").unwrap();
    std::fs::write(source.path().join("nested/new.txt"), "nested").unwrap();

    let overlay = tempfile::tempdir().unwrap();
    std::fs::write(overlay.path().join("existing.txt"), "user").unwrap();
    std::fs::create_dir_all(overlay.path().join("nested")).unwrap();
    std::fs::write(overlay.path().join("nested/existing.txt"), "user nested").unwrap();
    copy_directory_overlay(source.path(), overlay.path(), &HashSet::new()).unwrap();
    assert_eq!(
        std::fs::read_to_string(overlay.path().join("existing.txt")).unwrap(),
        "seed"
    );
    assert_eq!(
        std::fs::read_to_string(overlay.path().join("added.txt")).unwrap(),
        "added"
    );
    assert_eq!(
        std::fs::read_to_string(overlay.path().join("nested/existing.txt")).unwrap(),
        "seed nested"
    );
    assert_eq!(
        std::fs::read_to_string(overlay.path().join("nested/new.txt")).unwrap(),
        "nested"
    );

    let top_up = tempfile::tempdir().unwrap();
    std::fs::write(top_up.path().join("existing.txt"), "user").unwrap();
    std::fs::create_dir_all(top_up.path().join("nested")).unwrap();
    std::fs::write(top_up.path().join("nested/existing.txt"), "user nested").unwrap();
    top_up_missing_from(source.path(), top_up.path(), &HashSet::new()).unwrap();
    assert_eq!(
        std::fs::read_to_string(top_up.path().join("existing.txt")).unwrap(),
        "user"
    );
    assert_eq!(
        std::fs::read_to_string(top_up.path().join("added.txt")).unwrap(),
        "added"
    );
    assert_eq!(
        std::fs::read_to_string(top_up.path().join("nested/existing.txt")).unwrap(),
        "user nested"
    );
    assert_eq!(
        std::fs::read_to_string(top_up.path().join("nested/new.txt")).unwrap(),
        "nested"
    );
}
