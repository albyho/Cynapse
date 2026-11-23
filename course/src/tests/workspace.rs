use super::*;

fn package_with_workspace(workspace: &str) -> tempfile::TempDir {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["workspace"] = serde_json::json!(workspace);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    package
}

#[test]
fn course_workspace_resolves_from_the_package_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/multi-step-demo");
    let course = load_package(&root).unwrap();

    assert_eq!(course.workspace_path(), root.join("workspace"));
}

#[test]
fn missing_course_workspace_defaults_to_the_package_workspace() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let root = package.path();
    let course = load_package(root).unwrap();

    assert_eq!(course.workspace_path(), root.join("workspace"));
    assert_eq!(
        course.unit_workspace_path("ch/main"),
        Some(root.join("workspace/ch/main"))
    );
    assert_eq!(course.unit_workspace_path("missing"), None);
}

#[test]
fn a_workspace_path_that_is_a_file_is_rejected_even_when_omitted() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    std::fs::write(package.path().join("workspace"), "not a directory").unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("workspace"), "{error}");
}

#[test]
fn an_empty_declared_workspace_is_rejected() {
    let package = package_with_workspace("");

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("workspace path must not be empty"),
        "{error}"
    );
}

#[test]
fn a_workspace_at_the_package_root_is_rejected() {
    for workspace in [".", "./"] {
        let package = package_with_workspace(workspace);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(
            error.contains("workspace path segment"),
            "workspace {workspace:?} was not rejected: {error}"
        );
    }
}

#[test]
fn a_workspace_outside_the_package_workspace_is_rejected() {
    for workspace in [
        "build/ws",
        "steps",
        "seeds",
        "resources",
        "assets",
        "Steps",
        "steps/nested",
    ] {
        let package = package_with_workspace(workspace);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(
            error.contains("must stay inside the Course Package `workspace/` directory"),
            "workspace {workspace:?} was not rejected: {error}"
        );
    }
}

#[test]
fn a_workspace_with_an_unsafe_segment_is_rejected() {
    for workspace in [".git", "con", "a//b", "workspace/.tmp", "trailing."] {
        let package = package_with_workspace(workspace);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(
            error.contains("workspace path segment"),
            "workspace {workspace:?} was not rejected: {error}"
        );
    }
}

#[test]
fn a_resolved_workspace_inside_a_reserved_directory_is_detected() {
    let root = Path::new("course-root");
    assert!(crate::loader::resolves_into_reserved_dir(
        root,
        &root.join("steps/nested")
    ));
    assert!(crate::loader::resolves_into_reserved_dir(
        root,
        &root.join("STEPS")
    ));
    assert!(!crate::loader::resolves_into_reserved_dir(
        root,
        &root.join("workspace/unit")
    ));
    assert!(!crate::loader::resolves_into_reserved_dir(root, root));
}

#[test]
fn a_custom_nested_workspace_inside_the_workspace_directory_resolves() {
    let package = package_with_workspace("workspace/build/ws");

    let course = load_package(package.path()).unwrap();

    assert_eq!(
        course.workspace_path(),
        package.path().join("workspace/build/ws")
    );
}

#[test]
fn a_unit_seed_is_derived_from_its_own_path() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::create_dir_all(directory.path().join("seeds/ch/unit/child")).unwrap();
    std::fs::create_dir_all(directory.path().join("seeds/ch/unit/own")).unwrap();
    std::fs::write(
        directory.path().join("seeds/ch/unit/own/unit.txt"),
        "unit\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("seeds/ch/unit/child/child.txt"),
        "child\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
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
                        "title": "Child",
                        "steps": [{"step_file": "steps/step.json"}]
                    }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("steps/step.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{"type": "CreateFile", "path": "result.txt"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let course = load_package(directory.path()).unwrap();

    // 没有根层或祖先层，只有本 Unit 自己的目录。
    assert_eq!(
        course.workspace_seed_paths_for_path("ch/unit"),
        vec![directory.path().join("seeds/ch/unit")]
    );
    assert_eq!(
        course.workspace_seed_paths_for_path("ch/unit/child"),
        vec![directory.path().join("seeds/ch/unit/child")]
    );
    assert!(
        course
            .workspace_seed_exclusions_for_path("ch/unit")
            .contains(&directory.path().join("seeds/ch/unit/child"))
    );
    assert!(
        course
            .workspace_seed_exclusions_for_path("ch/unit/child")
            .is_empty()
    );
    assert!(course.workspace_seed_paths_for_path("unknown").is_empty());
}

#[test]
fn parent_seed_exclusions_cover_descendant_node_paths_with_different_dir_names() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::create_dir_all(directory.path().join("seeds/parent-dir/child")).unwrap();
    std::fs::create_dir_all(directory.path().join("seeds/parent-dir/child-dir")).unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "dir_name_collision",
            "title": "Dir Name Collision",
            "workspace": "workspace",
            "children": [{
                "id": "parent",
                "title": "Parent",
                "dir_name": "parent-dir",
                "steps": [{"step_file": "steps/step.json"}],
                "children": [{
                    "id": "child",
                    "title": "Child",
                    "dir_name": "child-dir",
                    "steps": [{"step_file": "steps/step.json"}]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("steps/step.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let course = load_package(directory.path()).unwrap();
    let exclusions = course.workspace_seed_exclusions_for_path("parent");

    assert!(exclusions.contains(&directory.path().join("seeds/parent-dir/child")));
    assert!(exclusions.contains(&directory.path().join("seeds/parent-dir/child-dir")));
}

#[test]
fn a_seed_directory_with_only_descendants_is_not_a_seed() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::create_dir_all(directory.path().join("seeds/ch/container/leaf")).unwrap();
    std::fs::write(
        directory.path().join("seeds/ch/container/leaf/leaf.txt"),
        "leaf\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "container_seed",
            "title": "Container Seed",
            "workspace": "workspace",
            "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [{
                    "id": "container",
                    "title": "Container",
                    "steps": [{"step_file": "steps/step.json"}],
                    "children": [{
                        "id": "leaf",
                        "title": "Leaf",
                        "steps": [{"step_file": "steps/step.json"}]
                    }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("steps/step.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let course = load_package(directory.path()).unwrap();

    assert!(
        course
            .workspace_seed_paths_for_path("ch/container")
            .is_empty()
    );
    assert_eq!(
        course.workspace_seed_paths_for_path("ch/container/leaf"),
        vec![directory.path().join("seeds/ch/container/leaf")]
    );
}

#[test]
fn resource_aliases_resolve_to_course_ancestor_and_node_roots() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::create_dir_all(directory.path().join("resources/ch/unit")).unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "resource_aliases",
            "title": "Resource Aliases",
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
            directory.path().join("steps/step.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "step_id": "step",
                "title": "Step",
                "actions": [{
                    "type": "TerminalCommand",
                    "session_key": "main",
                    "cwd": ".",
                    "command": "tool --course \"{{resource:course}}\" --ancestor \"{{resource:ancestor}}\" --node \"{{resource:node}}\""
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    let course = load_package(directory.path()).unwrap();

    let resolved = course
            .resolve_resource_aliases_for_path(
                "tool --course \"{{resource:course}}\" --ancestor \"{{resource:ancestor}}\" --node \"{{resource:node}}\"",
                "ch/unit",
            )
            .unwrap();

    for root in ["resources", "resources/ch", "resources/ch/unit"] {
        let expected = directory.path().join(root).canonicalize().unwrap();
        let expected = expected.to_string_lossy();
        let expected = expected.strip_prefix(r"\\?\").unwrap_or(&expected);
        assert!(resolved.contains(expected));
    }
    assert!(!resolved.contains("{{resource:"));
    assert!(!resolved.contains(r"\\?\"));
}

#[test]
fn fixture_exposes_unit_scoped_seed_and_resource_scopes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/resources-and-seeds-demo");
    let course = load_package(&root).unwrap();

    assert_eq!(course.tree.children.len(), 2);
    assert_eq!(
        course.workspace_seed_paths_for_path("ch_foundation/c_basics"),
        vec![root.join("seeds/ch_foundation/c_basics")]
    );
    assert_eq!(
        course.workspace_seed_paths_for_path("ch_advanced/model_demo"),
        vec![root.join("seeds/ch_advanced/model_demo")]
    );
    // 没有自己的 Seed 的 Unit 不继承课程级或节点级 Seed。
    assert!(
        course
            .workspace_seed_paths_for_path("ch_advanced/empty_unit")
            .is_empty()
    );
    assert_eq!(
        course.effective_adapters_for_path("ch_foundation/c_basics"),
        vec![AdapterId::Vscode]
    );
}
