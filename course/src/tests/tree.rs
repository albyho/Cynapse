use super::*;

#[test]
fn multi_level_fixture_exposes_grouping_nodes_and_units() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/multi-level-demo");
    let course = load_package(&root).unwrap();
    assert_eq!(course.tree.children.len(), 6);
    assert_eq!(course.units.len(), 12);
    assert!(course.units.iter().all(|unit| !unit.unit.steps.is_empty()));
    assert_eq!(course.playback_steps().len(), 12);
}

#[test]
fn flexible_tree_fixture_exposes_root_units_and_nested_groups() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/flexible-tree-demo");
    let course = load_package(&root).unwrap();

    assert_eq!(course.manifest.id, "node_abstraction");
    assert_eq!(course.manifest.title, "《Node 抽象》");
    assert_eq!(course.tree.id, "node_abstraction");
    assert!(course.tree.children[0].children.is_empty());
    assert!(course.tree.children[0].has_steps());
    assert!(!course.tree.children[1].children.is_empty());
    assert_eq!(
        course
            .units
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "intro",
            "ch01_basics/what_is_node",
            "ch01_basics/tree_and_recursion",
            "ch02_advanced/discriminated_union",
            "ch02_advanced/ch02_02_abstraction/structural_role",
            "ch02_advanced/ch02_02_abstraction/business_semantics",
            "appendix",
        ]
    );

    let intro = course.unit_by_path("intro").unwrap();
    assert_eq!(intro.unit.title, "简介");
    assert_eq!(intro.parent_id, "node_abstraction");
    assert_eq!(intro.parent_title, "《Node 抽象》");

    let structural = course
        .unit_by_path("ch02_advanced/ch02_02_abstraction/structural_role")
        .unwrap();
    assert_eq!(structural.unit.title, "2.2.1 结构角色");
    assert_eq!(structural.parent_id, "ch02_02_abstraction");
    assert_eq!(structural.parent_title, "2.2 抽象层次");

    assert_eq!(course.steps.len(), 7);
    assert_eq!(course.steps[0].unit_path, "intro");
}

#[test]
fn tree_unit_paths_drive_workspace_seed_and_resource_scopes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/flexible-tree-demo");
    let course = load_package(&root).unwrap();

    // Seed 只按 Unit Path 取本 Unit 的目录，不叠加课程级/祖先级层。
    assert_eq!(
        course.workspace_seed_paths_for_path("ch02_advanced/ch02_02_abstraction/structural_role"),
        vec![root.join("seeds/ch02_advanced/ch02_02_abstraction/structural_role")]
    );
    assert_eq!(
        course.workspace_seed_paths_for_path("intro"),
        vec![root.join("seeds/intro")]
    );
    assert_eq!(
        course.workspace_seed_paths_for_path("appendix"),
        vec![root.join("seeds/appendix")]
    );
    // 分组节点只是路径容器，没有自己的 Seed 文件。
    assert!(
        course
            .workspace_seed_paths_for_path("ch02_advanced/ch02_02_abstraction/business_semantics")
            .is_empty()
    );
    assert_eq!(
        course.unit_workspace_path("intro"),
        Some(root.join("workspace").join("intro"))
    );

    let resources = root.join("resources").canonicalize().unwrap();
    let intro_resources = resources.join("intro");
    let display = |path: &std::path::Path| {
        let path = path.to_string_lossy();
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
    };
    let resolved_intro = course
        .resolve_resource_aliases_for_path(
            "ancestor={{resource:ancestor}}|node={{resource:node}}",
            "intro",
        )
        .unwrap();
    assert_eq!(
        resolved_intro,
        format!(
            "ancestor={}|node={}",
            display(&resources),
            display(&intro_resources)
        )
    );

    let nested_resources = resources.join("ch02_advanced/ch02_02_abstraction");
    let nested_node_resources = nested_resources.join("structural_role");
    let resolved_nested = course
        .resolve_resource_aliases_for_path(
            "ancestor={{resource:ancestor}}|node={{resource:node}}",
            "ch02_advanced/ch02_02_abstraction/structural_role",
        )
        .unwrap();
    assert_eq!(
        resolved_nested,
        format!(
            "ancestor={}|node={}",
            display(&nested_resources),
            display(&nested_node_resources)
        )
    );
}

#[test]
fn unsafe_windows_path_segments_are_rejected() {
    for id in [
        "con",
        "NUL.txt",
        ".cynapse-resources-tmp",
        " leading",
        "trailing.",
        "trailing ",
        "bad:name",
        "bad?name",
        "bad\nname",
    ] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "unsafe_path",
                "title": "Unsafe Path",
                "children": [{
                    "id": id,
                    "title": "Unsafe",
                    "steps": [{"step_file": "steps/one.json"}]
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let error = load_package(directory.path()).unwrap_err().to_string();

        assert!(
            error.contains(&format!("id `{id}` must be a single safe path segment")),
            "id {id:?} was not rejected with a locatable error: {error}"
        );
    }
}

#[test]
fn sibling_ids_that_differ_only_by_case_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::write(
        directory.path().join("steps/one.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "one",
            "title": "One",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "case_insensitive_siblings",
            "title": "Case Insensitive Siblings",
            "children": [{
                "id": "A",
                "title": "Upper",
                "steps": [{"step_file": "steps/one.json"}]
            }, {
                "id": "a",
                "title": "Lower",
                "steps": [{"step_file": "steps/one.json"}]
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = load_package(directory.path()).unwrap_err().to_string();

    assert!(
        error.contains("node id `a` appears more than once under `course root`"),
        "{error}"
    );
}

#[test]
fn case_variant_ids_under_different_parents_coexist() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::write(
        directory.path().join("steps/one.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "one",
            "title": "One",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "case_variant_paths",
            "title": "Case Variant Paths",
            "children": [{
                "id": "upper",
                "title": "Upper",
                "children": [{
                    "id": "Unit",
                    "title": "Upper Unit",
                    "steps": [{"step_file": "steps/one.json"}]
                }]
            }, {
                "id": "lower",
                "title": "Lower",
                "children": [{
                    "id": "unit",
                    "title": "Lower Unit",
                    "steps": [{"step_file": "steps/one.json"}]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let course = load_package(directory.path()).unwrap();

    assert_eq!(
        course
            .units
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["upper/Unit", "lower/unit"]
    );
    assert!(course.node_by_path("upper/Unit").is_some());
    assert!(course.node_by_path("upper/unit").is_none());
    assert!(course.node_by_path("UPPER/Unit").is_none());
}

#[test]
fn a_root_node_named_resources_is_rejected() {
    for id in ["resources", "Resources", "RESOURCES"] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("steps")).unwrap();
        std::fs::write(
            directory.path().join("steps/one.json"),
            serde_json::to_vec(&serde_json::json!({
                "step_id": "one",
                "title": "One",
                "actions": [{"type": "Present"}]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "reserved_root",
                "title": "Reserved Root",
                "children": [{
                    "id": id,
                    "title": "Resources",
                    "steps": [{"step_file": "steps/one.json"}]
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let error = load_package(directory.path()).unwrap_err().to_string();

        assert!(
            error.contains(&format!("root node id `{id}` is reserved")),
            "root node id {id:?} was not rejected: {error}"
        );
    }
}

#[test]
fn invalid_course_trees_are_rejected() {
    fn load_tree(manifest: serde_json::Value) -> Result<Course, CourseError> {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        load_package(directory.path())
    }

    let cases = vec![
        (
            serde_json::json!({
                "id": "empty_root",
                "title": "Empty Root",
                "children": []
            }),
            "course root must contain at least one",
        ),
        (
            serde_json::json!({
                "id": "empty_group",
                "title": "Empty Grouping",
                "children": [{
                    "id": "ch",
                    "title": "Grouping",
                    "children": []
                }]
            }),
            "node `ch` declares neither steps nor children",
        ),
        (
            serde_json::json!({
                "id": "empty_unit",
                "title": "Empty Unit",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": []
                }]
            }),
            "node `unit` declares neither steps nor children",
        ),
        (
            serde_json::json!({
                "id": "duplicate_siblings",
                "title": "Duplicate Siblings",
                "children": [{
                    "id": "same",
                    "title": "First",
                    "steps": [{"step_file": "steps/one.json"}]
                }, {
                    "id": "same",
                    "title": "Second",
                    "steps": [{"step_file": "steps/two.json"}]
                }]
            }),
            "node id `same` appears more than once under `course root`",
        ),
        (
            serde_json::json!({
                "id": "unsafe_id",
                "title": "Unsafe Id",
                "children": [{
                    "id": "../escape",
                    "title": "Escape",
                    "steps": [{"step_file": "steps/one.json"}]
                }]
            }),
            "id `../escape` must be a single safe path segment",
        ),
    ];

    for (manifest, expected) in cases {
        let error = load_tree(manifest).unwrap_err().to_string();
        assert!(
            error.contains(expected),
            "expected `{expected}` in error: {error}"
        );
    }
}

#[test]
fn invalid_unit_shapes_are_rejected() {
    fn load_manifest(node: serde_json::Value) -> Result<Course, CourseError> {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                    "id": "invalid_nodes",
                    "title": "Invalid Nodes",
            "children": [node]
                }))
            .unwrap(),
        )
        .unwrap();
        load_package(directory.path())
    }

    assert!(matches!(
        load_manifest(serde_json::json!({
            "id": "ch",
            "title": "Grouping",
            "children": []
        })),
        Err(CourseError::Package(_))
    ));
    assert!(matches!(
        load_manifest(serde_json::json!({
            "id": "ch",
            "title": "Grouping",
            "children": [{
                "id": "empty",
                "title": "Empty",
                "steps": []
            }]
        })),
        Err(CourseError::Package(_))
    ));
    assert!(matches!(
        load_manifest(serde_json::json!({
            "id": "ch",
            "title": "Grouping",
            "children": [
                {
                    "id": "duplicate",
                    "title": "First",
                    "steps": [{"step_file": "steps/one.json"}]
                },
                {
                    "id": "duplicate",
                    "title": "Second",
                    "steps": [{"step_file": "steps/one.json"}]
                }
            ]
        })),
        Err(CourseError::Package(_))
    ));
}

#[test]
fn step_asset_and_environment_ids_are_case_sensitive() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let second_step = package.path().join("steps/case/Write_Hello.json");
    std::fs::create_dir_all(second_step.parent().unwrap()).unwrap();
    std::fs::write(
        &second_step,
        serde_json::to_vec(&serde_json::json!({
            "step_id": "Write_Hello",
            "title": "Write Hello",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();
    add_unit_audio_asset(&package, "Narration", "assets/narration/case-sensitive.ogg");
    add_unit_audio_asset(&package, "narration", "assets/narration/case-sensitive.ogg");

    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert(
        "steps".to_string(),
        serde_json::json!([
            {"step_file": "steps/write_hello.json"},
            {"step_file": "steps/case/Write_Hello.json"}
        ]),
    );
    unit.insert(
        "environment".to_string(),
        serde_json::json!([
            {"id": "rustc", "command": "rustc --version"},
            {"id": "RustC", "command": "rustc --version"}
        ]),
    );
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let course = load_package(package.path()).unwrap();
    let unit = &course.units[0].unit;

    assert_eq!(
        unit.environment
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect::<Vec<_>>(),
        ["rustc", "RustC"]
    );
    assert_eq!(
        unit.assets
            .iter()
            .map(|asset| asset.id.as_str())
            .collect::<Vec<_>>(),
        ["Narration", "narration"]
    );
    assert_eq!(
        course
            .steps
            .iter()
            .map(|step| step.step.step_id.as_str())
            .collect::<Vec<_>>(),
        ["write_hello", "Write_Hello"]
    );
}
