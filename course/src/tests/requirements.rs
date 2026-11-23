use super::*;

#[test]
fn terminal_actions_default_to_safe_execution_limits() {
    let ensure: Action = serde_json::from_value(serde_json::json!({
        "type": "TerminalEnsure",
        "session_key": "main",
        "cwd": "."
    }))
    .unwrap();
    let Action::TerminalEnsure(ensure) = ensure else {
        panic!("expected TerminalEnsure");
    };
    assert_eq!(ensure.session_key, "main");
    assert_eq!(ensure.cwd, ".");

    let command: Action = serde_json::from_value(serde_json::json!({
        "type": "TerminalCommand",
        "session_key": "main",
        "cwd": "hello_world",
        "command": "rustc main.rs"
    }))
    .unwrap();
    let Action::TerminalCommand(command) = command else {
        panic!("expected TerminalCommand");
    };
    assert_eq!(command.accepted_exit_codes, vec![0]);
    assert!(command.expected_output.is_empty());
    assert_eq!(command.timeout_ms, 300_000);
    assert!(command.completion_marker.is_none());
    assert!(!command.new_terminal);
}

#[test]
fn terminal_command_omits_an_absent_completion_marker() {
    let action = Action::TerminalCommand(TerminalCommandAction {
        session_key: "main".to_string(),
        cwd: "hello_world".to_string(),
        command: "rustc main.rs".to_string(),
        accepted_exit_codes: vec![0],
        expected_output: Vec::new(),
        timeout_ms: 300_000,
        completion_marker: None,
        new_terminal: false,
    });

    let payload = serde_json::to_value(action).unwrap();
    assert!(
        !payload
            .as_object()
            .unwrap()
            .contains_key("completion_marker")
    );
}

#[test]
fn environment_requirements_deserialize_generically() {
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "id": "rust",
        "title": "Rust",
        "environment": [
            {
                "id": "rustc",
                "command": "rustc --version",
                "minimum_version": "1.85.0",
                "install_hint": "Install Rust 1.85 or newer from rustup.rs"
            }
        ],
        "children": []
    }))
    .unwrap();

    let RequirementOverride::Values(environment) = &manifest.environment else {
        panic!("environment requirements were not parsed");
    };
    assert_eq!(environment.len(), 1);
    assert_eq!(environment[0].id, "rustc");
    assert_eq!(environment[0].minimum_version.as_deref(), Some("1.85.0"));
}

#[test]
fn adapter_requirements_distinguish_inheritance_and_override() {
    let default_manifest: Manifest = serde_json::from_value(serde_json::json!({
        "id": "default",
        "title": "Default",
        "children": []
    }))
    .unwrap();
    let adapter_manifest: Manifest = serde_json::from_value(serde_json::json!({
        "id": "vscode_course",
        "title": "VS Code Course",
        "adapters": ["vscode"],
        "children": []
    }))
    .unwrap();

    assert_eq!(default_manifest.adapters, RequirementOverride::Inherit);
    assert_eq!(
        adapter_manifest.adapters,
        RequirementOverride::Values(vec![AdapterId::Vscode])
    );
}

#[test]
fn legacy_course_and_requirement_fields_are_rejected() {
    let course_id_manifest = serde_json::json!({
        "course_id": "legacy",
        "title": "Legacy",
        "children": [{
            "id": "unit",
            "title": "Unit",
            "steps": [{"step_file": "steps/step.json"}]
        }]
    });
    assert!(serde_json::from_value::<Manifest>(course_id_manifest).is_err());

    let additive_requirements = serde_json::json!({
        "id": "legacy_requirements",
        "title": "Legacy Requirements",
        "children": [{
            "id": "unit",
            "title": "Unit",
            "adapter_additions": ["vscode"],
            "environment_additions": [{
                "id": "rustc",
                "command": "rustc --version"
            }],
            "steps": [{"step_file": "steps/step.json"}]
        }]
    });
    assert!(serde_json::from_value::<Manifest>(additive_requirements).is_err());
}

#[test]
fn node_and_unit_requirements_inherit_or_override_independently() {
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "id": "requirements",
        "title": "Requirements",
        "adapters": ["vscode"],
        "assets": [
            {"id": "root", "path": "assets/root.ogg"},
            {"id": "shared", "path": "assets/root-shared.ogg"}
        ],
        "environment": [{
            "id": "rustc",
            "command": "rustc --version"
        }],
        "children": [{
            "id": "ch",
            "title": "Grouping",
            "environment": [{
                "id": "python",
                "command": "python --version"
            }],
            "assets": [
                {"id": "ch", "path": "assets/grouping.ogg"},
                {"id": "shared", "path": "assets/grouping-shared.ogg"}
            ],
            "children": [
                {
                    "id": "inherited",
                    "title": "Inherited",
                    "assets": [
                        {"id": "unit", "path": "assets/unit.ogg"},
                        {"id": "shared", "path": "assets/unit-shared.ogg"}
                    ],
                    "steps": [{"step_file": "steps/step.json"}]
                },
                {
                    "id": "overridden",
                    "title": "Overridden",
                    "adapters": null,
                    "environment": [],
                    "steps": [{"step_file": "steps/step.json"}]
                }
            ]
        }, {
            "id": "disabled",
            "title": "Disabled",
            "adapters": [],
            "environment": null,
            "children": [{
                "id": "unit",
                "title": "Disabled Unit",
                "steps": [{"step_file": "steps/step.json"}]
            }]
        }]
    }))
    .unwrap();
    let tree = build_course_root(&manifest).unwrap();
    let units = collect_units(&tree);
    let course = Course {
        manifest,
        tree,
        units,
        steps: Vec::new(),
        package_root: PathBuf::new(),
    };

    assert_eq!(
        course.effective_adapters_for_path("ch/inherited"),
        vec![AdapterId::Vscode]
    );
    assert_eq!(
        course
            .effective_environment_requirements_for_path("ch/inherited")
            .into_iter()
            .map(|requirement| requirement.id)
            .collect::<Vec<_>>(),
        vec!["python"]
    );
    assert!(
        course
            .effective_adapters_for_path("ch/overridden")
            .is_empty()
    );
    assert!(
        course
            .effective_environment_requirements_for_path("ch/overridden")
            .is_empty()
    );
    assert!(
        course
            .effective_adapters_for_path("disabled/unit")
            .is_empty()
    );
    assert!(
        course
            .effective_environment_requirements_for_path("disabled/unit")
            .is_empty()
    );

    let inherited = course.unit_by_path("ch/inherited").expect("inherited unit");
    let assets = inherited
        .unit
        .assets
        .iter()
        .map(|asset| (asset.id.as_str(), asset.path.as_str()))
        .collect::<HashMap<_, _>>();
    assert_eq!(assets.get("root"), Some(&"assets/root.ogg"));
    assert_eq!(assets.get("ch"), Some(&"assets/grouping.ogg"));
    assert_eq!(assets.get("unit"), Some(&"assets/unit.ogg"));
    assert_eq!(
        assets.get("shared"),
        Some(&"assets/unit-shared.ogg"),
        "the nearest descendant asset must override the same id"
    );
}

#[test]
fn grouping_node_requirements_and_assets_apply_to_descendant_units() {
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "id": "inheritance",
        "title": "Inheritance",
        "adapters": ["vscode"],
        "assets": [
            {"id": "course", "path": "assets/course.ogg"},
            {"id": "replace", "path": "assets/course-replace.ogg"}
        ],
        "children": [{
            "id": "ch",
            "title": "Grouping",
            "adapters": null,
            "assets": [
                {"id": "ch", "path": "assets/grouping.ogg"},
                {"id": "replace", "path": "assets/grouping-replace.ogg"}
            ],
            "children": [{
                "id": "unit",
                "title": "Unit",
                "assets": [
                    {"id": "unit", "path": "assets/unit.ogg"},
                    {"id": "replace", "path": "assets/unit-replace.ogg"}
                ],
                "steps": [{"step_file": "steps/step.json"}]
            }]
        }]
    }))
    .unwrap();
    let tree = build_course_root(&manifest).unwrap();
    let units = collect_units(&tree);
    let unit = &units[0].unit;

    assert!(unit.adapters.is_empty());
    let assets = unit
        .assets
        .iter()
        .map(|asset| (asset.id.as_str(), asset.path.as_str()))
        .collect::<HashMap<_, _>>();
    assert_eq!(assets.get("course"), Some(&"assets/course.ogg"));
    assert_eq!(assets.get("ch"), Some(&"assets/grouping.ogg"));
    assert_eq!(assets.get("unit"), Some(&"assets/unit.ogg"));
    assert_eq!(
        assets.get("replace"),
        Some(&"assets/unit-replace.ogg"),
        "the nearest descendant asset must override the same id"
    );
}

#[test]
fn empty_or_null_asset_lists_preserve_inherited_assets() {
    for local_assets in [
        None,
        Some(serde_json::json!([])),
        Some(serde_json::Value::Null),
    ] {
        let package = write_test_package("steps/write_hello.json", self_contained_step());
        add_unit_audio_asset(
            &package,
            "narration_course_intro",
            "assets/narration/course/intro.ogg",
        );
        let manifest_path = package.path().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        let inherited_assets = {
            let unit = manifest["children"][0]["children"][0]
                .as_object_mut()
                .unwrap();
            let inherited_assets = unit.remove("assets").unwrap();
            if let Some(local_assets) = local_assets {
                unit.insert("assets".to_string(), local_assets);
            }
            inherited_assets
        };
        manifest["assets"] = inherited_assets;
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let course = load_package(package.path()).unwrap();
        let assets = course.units[0]
            .unit
            .assets
            .iter()
            .map(|asset| (asset.id.as_str(), asset.path.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            assets,
            [(
                "narration_course_intro",
                "assets/narration/course/intro.ogg"
            )]
        );
    }
}

#[test]
fn null_root_assets_are_empty_and_preserve_descendant_assets() {
    let empty_package = write_test_package("steps/write_hello.json", self_contained_step());
    let empty_manifest_path = empty_package.path().join("manifest.json");
    let mut empty_manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&empty_manifest_path).unwrap()).unwrap();
    empty_manifest["assets"] = serde_json::Value::Null;
    std::fs::write(
        &empty_manifest_path,
        serde_json::to_vec_pretty(&empty_manifest).unwrap(),
    )
    .unwrap();
    let empty_course = load_package(empty_package.path()).unwrap();
    assert!(
        empty_course
            .units
            .iter()
            .all(|unit| unit.unit.assets.is_empty()),
        "root null assets must not add local assets"
    );

    let package = write_test_package("steps/write_hello.json", self_contained_step());
    add_unit_audio_asset(
        &package,
        "narration_unit_intro",
        "assets/narration/unit/intro.ogg",
    );
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["assets"] = serde_json::Value::Null;
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let course = load_package(package.path()).unwrap();
    let assets = course.units[0]
        .unit
        .assets
        .iter()
        .map(|asset| (asset.id.as_str(), asset.path.as_str()))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        assets.get("narration_unit_intro"),
        Some(&"assets/narration/unit/intro.ogg"),
        "root null assets must preserve descendant assets"
    );
}

#[test]
fn duplicate_selected_requirements_are_rejected() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["children"][0]["children"][0]["adapters"] = serde_json::json!(["vscode", "vscode"]);
    manifest["children"][0]["children"][0]["environment"] = serde_json::json!([
        {
            "id": "rustc",
            "command": "rustc --version"
        },
        {
            "id": "rustc",
            "command": "rustc --version"
        }
    ]);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert!(matches!(
        load_package(package.path()),
        Err(CourseError::Package(_))
    ));
}
