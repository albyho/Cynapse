use super::*;

#[test]
fn self_contained_json_step_supplies_playback_metadata() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());

    let course = load_package(package.path()).unwrap();
    let playback = course.playback_steps();

    assert_eq!(course.steps.len(), 1);
    assert_eq!(playback.len(), 1);
    assert_eq!(playback[0].step_id, "write_hello");
    assert_eq!(playback[0].step_title, "Write Hello");
    assert!(matches!(
        playback[0].actions[0].action,
        Action::CreateFile(_)
    ));
}

#[test]
fn declared_course_workspace_must_stay_inside_the_package() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["workspace"] = serde_json::json!("../outside");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("workspace `../outside` is outside the Course Package"),
        "{error}"
    );
}

#[test]
fn an_absolute_workspace_path_is_rejected() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["workspace"] = serde_json::json!(if cfg!(windows) {
        "C:/outside" // test-isolation-allow: absolute path rejection fixture
    } else {
        "/outside"
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("is outside the Course Package"), "{error}");
}

#[test]
fn obsolete_workspace_seed_field_is_rejected() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["workspace_seed"] = serde_json::json!("seeds/shared");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("workspace_seed"), "{error}");
}

#[test]
fn step_files_cannot_escape_the_course_package() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    std::fs::create_dir_all(package.join("steps")).unwrap();
    std::fs::write(
        package.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "escape",
            "title": "Escape",
        "children": [{
                    "id": "ch",
                    "title": "Grouping",
                    "children": [{
                        "id": "main",
                        "title": "Main Unit",
                        "steps": [{ "step_file": "../outside.json" }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("outside.json"),
        serde_json::to_vec(&self_contained_step()).unwrap(),
    )
    .unwrap();

    let error = load_package(&package).unwrap_err().to_string();

    assert!(error.contains("outside the Course Package"), "{error}");
}

#[test]
fn step_files_must_use_json_and_match_their_step_id() {
    let old_extension = write_test_package("steps/write_hello.cyn", self_contained_step());
    let mismatched_name = write_test_package("steps/other_name.json", self_contained_step());

    let extension_error = load_package(old_extension.path()).unwrap_err().to_string();
    let mismatch_error = load_package(mismatched_name.path())
        .unwrap_err()
        .to_string();

    assert!(
        extension_error.contains("must use the .json extension"),
        "{extension_error}"
    );
    assert!(
        mismatch_error.contains("must match its step_id"),
        "{mismatch_error}"
    );
}

#[test]
fn a_node_with_empty_steps_and_no_children_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "legacy",
            "title": "Legacy",
        "children": [{
                    "id": "ch",
                    "title": "Grouping",
                    "steps": []
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    assert!(load_package(directory.path()).is_err());
}

#[test]
fn empty_and_null_steps_and_children_follow_the_absence_rule() {
    fn manifest(steps: serde_json::Value, children: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": "shapes",
            "title": "Shapes",
            "children": [{
                "id": "node",
                "title": "Node",
                "steps": steps,
                "children": children
            }]
        })
    }

    fn write_package(manifest: &serde_json::Value) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("steps")).unwrap();
        std::fs::write(
            directory.path().join("steps/only.json"),
            serde_json::to_vec(&serde_json::json!({
                "step_id": "only",
                "title": "Only",
                "actions": [{ "type": "Present" }]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(manifest).unwrap(),
        )
        .unwrap();
        directory
    }

    let steps = serde_json::json!([{ "step_file": "steps/only.json" }]);
    let children = serde_json::json!([{
        "id": "child",
        "title": "Child",
        "steps": [{ "step_file": "steps/only.json" }]
    }]);
    let empty = serde_json::json!([]);
    let null = serde_json::Value::Null;

    for (steps_value, children_value) in [
        (&empty, &empty),
        (&empty, &null),
        (&null, &empty),
        (&null, &null),
    ] {
        let directory = write_package(&manifest(steps_value.clone(), children_value.clone()));
        let error = load_package(directory.path()).unwrap_err().to_string();
        assert!(
            error.contains("declares neither steps nor children"),
            "steps={steps_value} children={children_value}: {error}"
        );
    }

    for (steps_value, children_value) in [
        (&steps, &empty),
        (&steps, &null),
        (&empty, &children),
        (&null, &children),
    ] {
        let directory = write_package(&manifest(steps_value.clone(), children_value.clone()));
        assert!(
            load_package(directory.path()).is_ok(),
            "steps={steps_value} children={children_value} should load"
        );
    }

    for children_value in [serde_json::json!([]), serde_json::Value::Null] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "root",
                "title": "Root",
                "children": children_value
            }))
            .unwrap(),
        )
        .unwrap();

        let error = load_package(directory.path()).unwrap_err().to_string();

        assert!(error.contains("must contain at least one node"), "{error}");
    }
}

#[test]
fn legacy_module_manifest_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "legacy_modules",
            "title": "Legacy Modules",
            "modules": [{
                "module_id": "module",
                "title": "Module",
        "children": []
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = load_package(directory.path()).unwrap_err().to_string();

    assert!(error.contains("unknown field `modules`"), "{error}");
}

#[test]
fn duplicate_step_ids_within_a_unit_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::write(
        directory.path().join("steps/write_hello.json"),
        serde_json::to_vec(&self_contained_step()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "duplicates",
            "title": "Duplicates",
        "children": [{
                    "id": "ch",
                    "title": "Grouping",
                    "children": [{
                        "id": "main",
                        "title": "Main Unit",
                        "steps": [
                            { "step_file": "steps/write_hello.json" },
                            { "step_file": "steps/write_hello.json" }
                        ]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = load_package(directory.path()).unwrap_err().to_string();

    assert!(error.contains("in unit `ch/main`"), "{error}");
}

#[test]
fn units_group_ordered_steps_and_allow_cross_unit_reuse() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::write(
        directory.path().join("steps/shared.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "shared",
            "title": "Shared Step",
            "actions": [{
                "type": "CreateFile",
                "path": "shared.txt"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "leaves",
            "title": "Leaves",
            "adapters": ["vscode"],
        "children": [{
                    "id": "ch",
                    "title": "Grouping",
                    "children": [
                        {
                            "id": "first_unit",
                            "title": "First Unit",
                            "steps": [
                                { "step_file": "steps/shared.json" }
                            ]
                        },
                        {
                            "id": "second_unit",
                            "title": "Second Unit",
                            "steps": [
                                { "step_file": "steps/shared.json" }
                            ]
                        }
                    ]
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let course = load_package(directory.path()).unwrap();
    let playback = course.playback_steps();

    assert_eq!(course.units.len(), 2);
    assert_eq!(playback.len(), 2);
    assert_eq!(playback[0].unit_path, "ch/first_unit");
    assert_eq!(playback[0].unit_title, "First Unit");
    assert_eq!(playback[1].unit_path, "ch/second_unit");
    assert_eq!(playback[0].step_id, playback[1].step_id);
}

#[test]
fn declared_workspace_reset_paths_are_rejected() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["children"][0]["children"][0]["workspace_reset"] = serde_json::json!(["src"]);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert!(matches!(
        load_package(package.path()),
        Err(CourseError::Json(_))
    ));
}

#[test]
fn legacy_step_reference_fields_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::write(
        directory.path().join("steps/write_hello.json"),
        serde_json::to_vec(&self_contained_step()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "legacy_reference",
            "title": "Legacy Reference",
        "children": [{
                    "id": "ch",
                    "title": "Grouping",
                    "children": [{
                        "id": "main",
                        "title": "Main Unit",
                        "steps": [{
                            "step_file": "steps/write_hello.json",
                            "cyn_file": "steps/write_hello.cyn"
                        }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    assert!(load_package(directory.path()).is_err());
}

#[test]
fn empty_course_tree_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "empty_course",
            "title": "Empty Course",
        "children": []
        }))
        .unwrap(),
    )
    .unwrap();

    assert!(load_package(directory.path()).is_err());
}

#[test]
fn node_dependencies_are_rejected() {
    for manifest in [
        serde_json::json!({
            "id": "node_dependency",
            "title": "Grouping Dependency",
        "children": [{
                "id": "ch",
                "title": "Grouping",
                "depends_on": ["previous"],
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": [{"step_file": "steps/step.json"}]
                }]
            }]
        }),
        serde_json::json!({
            "id": "unit_dependency",
            "title": "Unit Dependency",
        "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "depends_on": ["previous"],
                    "steps": [{"step_file": "steps/step.json"}]
                }]
            }]
        }),
    ] {
        assert!(serde_json::from_value::<Manifest>(manifest).is_err());
    }
}
