use cynapse_client_core::{CourseSession, CourseTreeNodeView, UnitStatus, load_package};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const DEEP_UNIT: &str = "ch02_advanced/ch02_02_abstraction/structural_role";

fn fixture(name: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shared = manifest.join("../../course/test-data").join(name);
    if shared.is_dir() {
        shared
    } else {
        manifest.join("../test-data").join(name)
    }
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

fn top_level_node_by_path<'a>(
    nodes: &'a [CourseTreeNodeView],
    path: &str,
) -> &'a CourseTreeNodeView {
    nodes
        .iter()
        .find(|node| node.path == path)
        .unwrap_or_else(|| panic!("node {path} not found"))
}

#[test]
fn node_completion_is_derived_from_descendant_units_and_survives_restart() {
    let package = tempfile::tempdir().unwrap();
    copy_fixture("flexible-tree-demo", package.path());
    let state = tempfile::tempdir().unwrap();
    let db_path = state.path().join("state.db");

    let mut session = CourseSession::new(&db_path).unwrap();
    let opened = session.open_course(package.path()).unwrap();
    let course = opened.active_course.unwrap();
    let chapter = top_level_node_by_path(&course.tree, "ch01_basics");
    assert!(!chapter.completed);

    for (unit_path, step_id) in [
        ("ch01_basics/what_is_node", "what_is_node"),
        ("ch01_basics/tree_and_recursion", "tree_and_recursion"),
    ] {
        session.start_unit(unit_path, true).unwrap();
        session.record_progress(unit_path, step_id, true).unwrap();
    }
    let completed = session.state().active_course.unwrap();
    assert!(top_level_node_by_path(&completed.tree, "ch01_basics").completed);
    assert!(
        !completed
            .tree
            .iter()
            .find(|node| node.path == "intro")
            .unwrap()
            .completed
    );
    drop(session);

    let restored = CourseSession::new(&db_path).unwrap().state();
    let restored_course = restored.active_course.unwrap();
    assert!(top_level_node_by_path(&restored_course.tree, "ch01_basics").completed);
}

#[test]
fn deep_unit_continues_and_resumes_by_unit_path() {
    let package = tempfile::tempdir().unwrap();
    copy_fixture("flexible-tree-demo", package.path());
    let state = tempfile::tempdir().unwrap();
    let db_path = state.path().join("state.db");

    let mut session = CourseSession::new(&db_path).unwrap();
    let opened = session.open_course(package.path()).unwrap();
    assert!(
        opened
            .active_course
            .as_ref()
            .unwrap()
            .tree
            .iter()
            .any(|node| !node.children.is_empty())
    );

    let started = session.start_unit(DEEP_UNIT, true).unwrap();
    assert_eq!(
        started
            .selected_unit
            .as_ref()
            .map(|unit| unit.path.as_str()),
        Some(DEEP_UNIT)
    );
    session
        .record_progress(DEEP_UNIT, "structural_role", false)
        .unwrap();
    let before_restart = session.playback_resume().unwrap();
    assert_eq!(before_restart.unit_path, DEEP_UNIT);
    assert_eq!(before_restart.step_id, "structural_role");
    drop(session);

    let restored = CourseSession::new(&db_path).unwrap();
    let restored_state = restored.state();
    let unit = restored_state.selected_unit.unwrap();
    assert_eq!(unit.path, DEEP_UNIT);
    let resume = restored.playback_resume().unwrap();
    assert_eq!(resume.unit_path, DEEP_UNIT);
    assert_eq!(resume.step_id, "structural_role");
}

#[test]
fn root_unit_starts_and_resumes_by_unit_path() {
    let package = tempfile::tempdir().unwrap();
    copy_fixture("flexible-tree-demo", package.path());
    let state = tempfile::tempdir().unwrap();
    let db_path = state.path().join("state.db");

    let mut session = CourseSession::new(&db_path).unwrap();
    session.open_course(package.path()).unwrap();
    let started = session.start_unit("intro", true).unwrap();
    assert_eq!(
        started
            .selected_unit
            .as_ref()
            .map(|unit| unit.path.as_str()),
        Some("intro")
    );
    session
        .record_progress("intro", "intro_overview", false)
        .unwrap();
    drop(session);

    let restored = CourseSession::new(&db_path).unwrap();
    let resume = restored.playback_resume().unwrap();
    assert_eq!(resume.unit_path, "intro");
    assert_eq!(resume.step_id, "intro_overview");
}

#[test]
fn nested_unit_workspace_and_root_resources_follow_course_tree_paths() {
    let package = tempfile::tempdir().unwrap();
    copy_fixture("flexible-tree-demo", package.path());
    let course = load_package(package.path()).unwrap();

    let deep_seeds = course.workspace_seed_paths_for_path(DEEP_UNIT);
    assert_eq!(
        deep_seeds,
        vec![
            package
                .path()
                .join("seeds/ch02_advanced/ch02_02_abstraction/structural_role")
        ]
    );
    assert_eq!(
        course.unit_workspace_path(DEEP_UNIT),
        Some(
            package
                .path()
                .join("workspace/ch02_advanced/ch02_02_abstraction/structural_role")
        )
    );
    let ancestor = package.path().join("resources").canonicalize().unwrap();
    let node = package
        .path()
        .join("resources/intro")
        .canonicalize()
        .unwrap();
    let display = |path: &std::path::Path| {
        let path = path.to_string_lossy();
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
    };
    assert_eq!(
        course
            .resolve_resource_aliases_for_path("{{resource:ancestor}}/{{resource:node}}", "intro")
            .unwrap(),
        format!("{}/{}", display(&ancestor), display(&node))
    );
}

#[test]
fn resetting_a_nested_unit_leaves_it_seedless_and_rebuilds_on_start() {
    let package = tempfile::tempdir().unwrap();
    fs::create_dir_all(package.path().join("steps")).unwrap();
    // 每个 Unit 只有自己的 Seed；`seeds/alpha/deep/ancestor.txt` 属于分组节点，
    // 没有任何 Unit 会继承它。
    for (path, contents) in [
        ("seeds/alpha/deep/ancestor.txt", "deep ancestor"),
        ("seeds/alpha/deep/unit/common.txt", "unit"),
        ("seeds/alpha/deep/sibling/sibling.txt", "sibling"),
    ] {
        let file = package.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, contents).unwrap();
    }
    fs::write(
        package.path().join("manifest.json"),
        serde_json::to_vec(&json!({
            "id": "nested_reset",
            "title": "Nested Reset",
            "workspace": "workspace",
            "children": [{
                "id": "alpha",
                "title": "Alpha",
                "children": [{
                    "id": "deep",
                    "title": "Deep",
                    "children": [{
                        "id": "unit",
                        "title": "Unit",
                        "steps": [{"step_file": "steps/unit.json"}]
                    }, {
                        "id": "sibling",
                        "title": "Sibling",
                        "steps": [{"step_file": "steps/sibling.json"}]
                    }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    for step_id in ["unit", "sibling"] {
        fs::write(
            package.path().join(format!("steps/{step_id}.json")),
            serde_json::to_vec(&json!({
                "step_id": step_id,
                "title": step_id,
                "actions": [{"type": "Present"}]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let state = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(package.path()).unwrap();
    session.start_unit("alpha/deep/unit", true).unwrap();
    let unit_workspace = package.path().join("workspace/alpha/deep/unit");
    assert_eq!(
        fs::read_to_string(unit_workspace.join("common.txt")).unwrap(),
        "unit"
    );
    // 分组节点的 Seed 不会被继承，后代 Unit 的 Seed 也不会混进来。
    assert!(!unit_workspace.join("ancestor.txt").exists());
    assert!(!unit_workspace.join("sibling").exists());
    session
        .record_progress("alpha/deep/unit", "unit", false)
        .unwrap();
    fs::write(unit_workspace.join("user.txt"), "remove me").unwrap();

    session.start_unit("alpha/deep/sibling", true).unwrap();
    let sibling_workspace = package.path().join("workspace/alpha/deep/sibling");
    fs::write(sibling_workspace.join("keep.txt"), "keep me").unwrap();

    session.reset_unit("alpha/deep/unit", true).unwrap();

    // 重置后该 Unit 的工作区保持“尚未开始”的空状态，兄弟 Unit 不受影响。
    assert!(!unit_workspace.join("user.txt").exists());
    assert!(!unit_workspace.exists());
    assert_eq!(
        fs::read_to_string(sibling_workspace.join("keep.txt")).unwrap(),
        "keep me"
    );

    // 下一次开始（或“继续”）时才从本 Unit 的 Seed 重建。
    session.start_unit("alpha/deep/unit", true).unwrap();
    assert_eq!(
        fs::read_to_string(unit_workspace.join("common.txt")).unwrap(),
        "unit"
    );
    assert!(!unit_workspace.join("ancestor.txt").exists());
    assert_eq!(
        fs::read_to_string(sibling_workspace.join("keep.txt")).unwrap(),
        "keep me"
    );
}

#[test]
fn parent_seed_cannot_write_into_a_descendant_workspace_with_a_different_dir_name() {
    let package = tempfile::tempdir().unwrap();
    fs::create_dir_all(package.path().join("steps")).unwrap();
    fs::create_dir_all(package.path().join("seeds/parent-dir")).unwrap();
    fs::create_dir_all(package.path().join("seeds/parent-dir/child-dir")).unwrap();
    fs::write(package.path().join("seeds/parent-dir/child"), "collision\n").unwrap();
    fs::write(
        package.path().join("seeds/parent-dir/child-dir/child.txt"),
        "child\n",
    )
    .unwrap();
    fs::write(
        package.path().join("manifest.json"),
        serde_json::to_vec(&json!({
            "id": "dir_name_collision",
            "title": "Dir Name Collision",
            "workspace": "workspace",
            "children": [{
                "id": "parent",
                "title": "Parent",
                "dir_name": "parent-dir",
                "steps": [{"step_file": "steps/present.json"}],
                "children": [{
                    "id": "child",
                    "title": "Child",
                    "dir_name": "child-dir",
                    "steps": [{"step_file": "steps/present.json"}]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        package.path().join("steps/present.json"),
        serde_json::to_vec(&json!({
            "step_id": "present",
            "title": "Present",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let state = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(package.path()).unwrap();
    session.start_unit("parent", true).unwrap();
    assert!(!package.path().join("workspace/parent/child").is_file());

    session.start_unit("parent/child", true).unwrap();
    assert_eq!(
        fs::read_to_string(package.path().join("workspace/parent/child/child.txt")).unwrap(),
        "child\n"
    );
}

#[test]
fn resetting_a_parent_unit_preserves_descendant_unit_workspaces() {
    let package = tempfile::tempdir().unwrap();
    fs::create_dir_all(package.path().join("steps")).unwrap();
    // `alpha` 自身是 Unit（有自己的步骤），同时含子 Unit `alpha/child`。
    // Seed 按 Unit 独立：`seeds/alpha/` 只属于 alpha，`seeds/alpha/child/` 只属于 child。
    for (path, contents) in [
        ("seeds/alpha/alpha_own.txt", "alpha own"),
        ("seeds/alpha/child/child_own.txt", "child own"),
    ] {
        let file = package.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, contents).unwrap();
    }
    fs::write(
        package.path().join("manifest.json"),
        serde_json::to_vec(&json!({
            "id": "parent_reset",
            "title": "Parent Reset",
            "workspace": "workspace",
            "children": [{
                "id": "alpha",
                "title": "Alpha",
                "steps": [{"step_file": "steps/alpha.json"}],
                "children": [{
                    "id": "child",
                    "title": "Child",
                    "steps": [{"step_file": "steps/child.json"}]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    for step_id in ["alpha", "child"] {
        fs::write(
            package.path().join(format!("steps/{step_id}.json")),
            serde_json::to_vec(&json!({
                "step_id": step_id,
                "title": step_id,
                "actions": [{"type": "Present"}]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let state = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(state.path().join("state.db")).unwrap();
    session.open_course(package.path()).unwrap();

    // 开始并推进父子两个 Unit，让两者的工作区都落到磁盘。
    session.start_unit("alpha", true).unwrap();
    session.start_unit("alpha/child", true).unwrap();
    let alpha_workspace = package.path().join("workspace/alpha");
    let child_workspace = package.path().join("workspace/alpha/child");
    assert_eq!(
        fs::read_to_string(alpha_workspace.join("alpha_own.txt")).unwrap(),
        "alpha own"
    );
    assert_eq!(
        fs::read_to_string(child_workspace.join("child_own.txt")).unwrap(),
        "child own"
    );
    // 子 Unit 工作区里有一个学员自己的文件，重置父 Unit 时不应被连累。
    fs::write(child_workspace.join("user.txt"), "keep me").unwrap();
    session.record_progress("alpha", "alpha", false).unwrap();
    session
        .record_progress("alpha/child", "child", false)
        .unwrap();

    let scope = session.reset_scope_for_unit("alpha").unwrap();
    assert_eq!(scope.exclude_paths, vec!["alpha/child".to_string()]);

    session.reset_unit("alpha", true).unwrap();

    // 父 Unit 自己的文件被清掉，但工作区目录仍在（因为仍装着子 Unit 目录）。
    assert!(!alpha_workspace.join("alpha_own.txt").exists());
    assert!(alpha_workspace.exists());
    // 子 Unit 的工作区原样保留：Seed 文件与学员文件都在。
    assert_eq!(
        fs::read_to_string(child_workspace.join("child_own.txt")).unwrap(),
        "child own"
    );
    assert_eq!(
        fs::read_to_string(child_workspace.join("user.txt")).unwrap(),
        "keep me"
    );

    // 子 Unit 的“已学过”进度（数据库）不被父 Unit 重置波及。
    let view = session
        .state()
        .active_course
        .expect("active course present");
    assert_eq!(
        find_unit_status(&view.tree, "alpha/child"),
        Some(UnitStatus::InProgress)
    );

    // 重置后重新开始父 Unit 时，Seed 替换不能把子工作区整目录删掉。
    session.start_unit("alpha", true).unwrap();
    assert_eq!(
        fs::read_to_string(alpha_workspace.join("alpha_own.txt")).unwrap(),
        "alpha own"
    );
    assert_eq!(
        fs::read_to_string(child_workspace.join("child_own.txt")).unwrap(),
        "child own"
    );
    assert_eq!(
        fs::read_to_string(child_workspace.join("user.txt")).unwrap(),
        "keep me"
    );
}

fn find_unit_status(nodes: &[CourseTreeNodeView], path: &str) -> Option<UnitStatus> {
    for node in nodes {
        if node.path == path {
            return node.unit.as_ref().map(|unit| unit.status);
        }
        if let Some(status) = find_unit_status(&node.children, path) {
            return Some(status);
        }
    }
    None
}
