use super::*;

#[test]
fn action_before_narration_loads_an_explicit_bounded_clip() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "CreateFile",
        "path": "hello.txt",
        "narration": {
            "before": {
                "asset_id": "narration",
                "clips": [{
                    "offset_ms": 500,
                    "duration_ms": 1200,
                    "caption": "先听完这句讲解。"
                }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/before.ogg");

    let course = load_package(package.path()).unwrap();
    let playback = course.playback_steps();
    let before = playback[0].actions[0]
        .narration
        .before
        .as_ref()
        .expect("Action Before Phase")
        .clips
        .first()
        .expect("Action Before Clip");

    assert_eq!(before.offset_ms, Some(500));
    assert_eq!(before.duration_ms, 1200);
    assert_eq!(before.caption.as_deref(), Some("先听完这句讲解。"));
    assert!(
        before
            .audio_path
            .as_ref()
            .is_some_and(|path| path.ends_with("assets/narration/before.ogg"))
    );
    assert_eq!(
        serde_json::to_value(&course.actions()[0]).unwrap(),
        serde_json::json!({"type": "CreateFile", "path": "hello.txt", "create_parents": true})
    );
}

#[test]
fn narration_phase_loads_an_ordered_clip_sequence_from_one_asset() {
    let mut step = self_contained_step();
    step["actions"][0]["narration"] = serde_json::json!({
        "during": {
            "asset_id": "narration",
            "clips": [{
                "offset_ms": 0,
                "duration_ms": 500,
                "caption": "第一段。"
            }, {
                "offset_ms": 1000,
                "duration_ms": 750,
                "caption": "第二段。"
            }]
        }
    });
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/phases.ogg");

    let course = load_package(package.path()).unwrap();
    let clips = course
        .action_narration("ch/main", "write_hello", 0, ActionNarrationPhase::During, 0)
        .expect("first clip");
    let second = course
        .action_narration("ch/main", "write_hello", 0, ActionNarrationPhase::During, 1)
        .expect("second clip");

    assert_eq!(clips.offset_ms, Some(0));
    assert_eq!(clips.duration_ms, 500);
    assert_eq!(clips.caption.as_deref(), Some("第一段。"));
    assert_eq!(second.offset_ms, Some(1000));
    assert_eq!(second.duration_ms, 750);
    assert_eq!(second.caption.as_deref(), Some("第二段。"));
    assert_eq!(clips.audio_relative_path, second.audio_relative_path);
}

#[test]
fn action_narration_rejects_audio_only_clips() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "CreateFile",
        "path": "audio-only.txt",
        "narration": {
            "before": {
                "asset_id": "narration",
                "clips": [{
                    "offset_ms": 0,
                    "duration_ms": 500
                }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/before.ogg");

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(error.contains("must declare a caption"), "{error}");
}

#[test]
fn narration_speech_edits_are_validated_and_ignored_at_runtime() {
    let mut step = self_contained_step();
    step["actions"][0]["narration"] = serde_json::json!({
        "before": {
            "clips": [{
                "duration_ms": 900,
                "caption": "输出 <c.aqua>{}。</c.aqua>",
                "speech_edits": [{
                    "i": 3,
                    "s": "{}",
                    "p": "一对大括号"
                }]
            }, {
                "duration_ms": 900,
                "caption": "无替换。",
                "speech_edits": null
            }, {
                "duration_ms": 900,
                "caption": "空词表。",
                "speech_edits": []
            }]
        }
    });
    let package = write_test_package("steps/write_hello.json", step);

    let course = load_package(package.path()).unwrap();
    let caption = course
        .action_narration("ch/main", "write_hello", 0, ActionNarrationPhase::Before, 0)
        .unwrap()
        .caption
        .clone();

    assert_eq!(caption.as_deref(), Some("输出 <c.aqua>{}。</c.aqua>"));
}

#[test]
fn narration_speech_edits_reject_invalid_spans() {
    for (edits, expected) in [
        (
            serde_json::json!([{"i": 1, "s": "{}", "p": "大括号"}]),
            "does not match",
        ),
        (
            serde_json::json!([{"i": 20, "s": "{}", "p": "大括号"}]),
            "outside the Caption",
        ),
        (
            serde_json::json!([
                {"i": 0, "s": "abc", "p": "x"},
                {"i": 1, "s": "bc", "p": "y"}
            ]),
            "overlap",
        ),
        (
            serde_json::json!([{"i": 0, "s": "abc", "p": " "}]),
            "p must not be empty",
        ),
    ] {
        let mut step = self_contained_step();
        step["actions"][0]["narration"] = serde_json::json!({
            "before": {
                "clips": [{
                    "duration_ms": 900,
                    "caption": "abc",
                    "speech_edits": edits
                }]
            }
        });
        let package = write_test_package("steps/write_hello.json", step);
        let error = load_package(package.path()).expect_err("invalid speech edit");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?}, got {error}"
        );
    }
}

#[test]
fn unit_audio_assets_reject_duplicate_ids_and_invalid_paths() {
    for (assets, expected) in [
        (
            serde_json::json!([
                {"id": "narration", "path": "assets/narration/before.ogg"},
                {"id": "narration", "path": "assets/narration/before.ogg"}
            ]),
            "duplicate asset id `narration`",
        ),
        (
            serde_json::json!([
                {"id": "narration", "path": "assets\\\\narration\\\\before.ogg"}
            ]),
            "must use forward slashes",
        ),
        (
            serde_json::json!([
                {"id": "narration", "path": "assets/narration/before.wav"}
            ]),
            "must use the .ogg extension",
        ),
        (
            serde_json::json!([
                {"id": "narration", "path": "assets/narration/before.vtt"}
            ]),
            "must use the .ogg extension",
        ),
        (
            serde_json::json!([
                {"id": "narration", "path": "../outside.ogg"}
            ]),
            "is outside the Course Package",
        ),
    ] {
        let package = write_test_package("steps/write_hello.json", self_contained_step());
        add_unit_audio_asset(&package, "narration", "assets/narration/before.ogg");
        let manifest_path = package.path().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["children"][0]["children"][0]["assets"] = assets;
        std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn narration_audio_fields_are_rejected_at_every_manifest_level() {
    for level in ["course", "ch", "unit"] {
        let package = write_test_package("steps/write_hello.json", self_contained_step());
        let manifest_path = package.path().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        let target = match level {
            "course" => manifest.as_object_mut().unwrap(),
            "ch" => manifest["children"][0].as_object_mut().unwrap(),
            "unit" => manifest["children"][0]["children"][0]
                .as_object_mut()
                .unwrap(),
            _ => unreachable!(),
        };
        target.insert(
            "narration_audio".to_string(),
            serde_json::json!("assets/narration/legacy.ogg"),
        );
        std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains("unknown field `narration_audio`"), "{error}");
    }

    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let step_path = package.path().join("steps/write_hello.json");
    let mut step: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&step_path).unwrap()).unwrap();
    step["narration_audio"] = serde_json::json!("assets/narration/legacy.ogg");
    std::fs::write(step_path, serde_json::to_vec_pretty(&step).unwrap()).unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("unknown field `narration_audio`"), "{error}");
}

#[test]
fn legacy_unit_narration_track_is_rejected() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["children"][0]["children"][0]["narration"] = serde_json::json!({
        "segments": [{
            "segment_id": "legacy",
            "captions": "assets/legacy.vtt"
        }],
        "step_gates": []
    });
    std::fs::create_dir_all(package.path().join("assets")).unwrap();
    std::fs::write(
        package.path().join("assets/legacy.vtt"),
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nlegacy\n",
    )
    .unwrap();
    std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("unknown field `narration`"), "{error}");
}

#[test]
fn action_before_narration_rejects_invalid_clips() {
    let cases = [
        (
            serde_json::json!({
                "asset_id": "narration",
                "clips": [{
                    "caption": "missing range"
                }]
            }),
            "offset_ms and duration_ms",
        ),
        (
            serde_json::json!({
                "asset_id": "narration",
                "clips": [{
                    "offset_ms": 0,
                    "duration_ms": 4_000,
                    "caption": "超出范围"
                }]
            }),
            "exceeds",
        ),
        (
            serde_json::json!({
                "asset_id": "narration",
                "clips": [{"offset_ms": 0, "duration_ms": 500}]
            }),
            "must declare a caption",
        ),
        (
            serde_json::json!({
                "clips": [{"caption": "", "duration_ms": 1_000}]
            }),
            "must declare a caption",
        ),
        (
            serde_json::json!({
                "asset_id": "missing",
                "clips": [{"offset_ms": 0, "duration_ms": 500}]
            }),
            "unknown asset_id `missing`",
        ),
        (
            serde_json::json!({
                "clips": [{
                    "offset_ms": 0,
                    "duration_ms": 500,
                    "caption": "offset without audio"
                }]
            }),
            "offset_ms requires audio",
        ),
        (
            serde_json::json!({
                "asset_id": " ",
                "clips": [{"offset_ms": 0, "duration_ms": 500}]
            }),
            "asset_id must not be empty",
        ),
        (
            serde_json::json!({
                "asset_id": null,
                "clips": [{
                    "duration_ms": 500,
                    "caption": "null is not the way to omit the field"
                }]
            }),
            "asset_id must be a string",
        ),
        (serde_json::json!({"clips": []}), "clips must not be empty"),
        (
            serde_json::json!({
                "asset_id": "narration",
                "clips": [
                    {
                        "offset_ms": 100,
                        "duration_ms": 500,
                        "caption": "第一段。"
                    },
                    {
                        "offset_ms": 500,
                        "duration_ms": 500,
                        "caption": "第二段。"
                    }
                ]
            }),
            "overlaps the previous clip",
        ),
    ];
    for (before, expected) in cases {
        let mut step = self_contained_step();
        step["actions"][0]["narration"] = serde_json::json!({ "before": before });
        let package = write_test_package("steps/write_hello.json", step);
        add_unit_audio_asset(&package, "narration", "assets/narration/before.ogg");

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn audio_asset_ids_are_scoped_to_their_unit() {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(package.path().join("steps")).unwrap();
    std::fs::create_dir_all(package.path().join("assets/narration")).unwrap();
    for name in ["first.ogg", "second.ogg"] {
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test-data/phase-timeline-demo/assets/narration/phases.ogg"),
            package.path().join("assets/narration").join(name),
        )
        .unwrap();
    }
    std::fs::write(
        package.path().join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "unit_audio",
            "title": "Unit Audio",
            "adapters": ["vscode"],
        "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [
                    {
                        "id": "first",
                        "title": "First",
                        "assets": [{
                            "id": "narration",
                            "path": "assets/narration/first.ogg"
                        }],
                        "steps": [{"step_file": "steps/write_hello.json"}]
                    },
                    {
                        "id": "second",
                        "title": "Second",
                        "assets": [{
                            "id": "narration",
                            "path": "assets/narration/second.ogg"
                        }],
                        "steps": [{"step_file": "steps/write_hello.json"}]
                    }
                ]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        package.path().join("steps/write_hello.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "write_hello",
            "title": "Write Hello",
            "actions": [{
                "type": "CreateFile",
                "path": "hello.txt",
                "narration": {
                    "before": {
                        "asset_id": "narration",
                        "clips": [{
                            "offset_ms": 0,
                            "duration_ms": 500,
                            "caption": "Unit-scoped narration."
                        }]
                    }
                }
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let course = load_package(package.path()).unwrap();

    for (unit_id, expected) in [
        ("first", "assets/narration/first.ogg"),
        ("second", "assets/narration/second.ogg"),
    ] {
        let before = course
            .action_narration(
                &format!("ch/{unit_id}"),
                "write_hello",
                0,
                ActionNarrationPhase::Before,
                0,
            )
            .expect("before narration");
        assert_eq!(
            before.audio_relative_path.as_deref(),
            Some(Path::new(expected))
        );
    }
}

#[test]
fn action_before_demo_reuses_one_audio_source_with_an_integer_offset() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/action-before-narration-demo");
    let course = load_package(&root).unwrap();
    let offset_step = course
        .playback_steps()
        .into_iter()
        .find(|step| step.unit_path == "before/offset_test")
        .expect("offset second Step");
    let before = &offset_step.actions[0]
        .narration
        .before
        .as_ref()
        .expect("Before Phase")
        .clips[0];

    assert_eq!(before.offset_ms, Some(5000));
    assert_eq!(before.duration_ms, 3309);
    assert_eq!(
        before.audio_relative_path.as_deref(),
        Some(Path::new("assets/narration/combined-narration.ogg"))
    );
    assert_eq!(
        course.audio_asset_paths(),
        vec![PathBuf::from("assets/narration/combined-narration.ogg")]
    );
}

#[test]
fn action_narration_loads_all_legal_phase_combinations() {
    let combinations = [
        serde_json::json!({ "before": narrate("before") }),
        serde_json::json!({ "during": narrate("during") }),
        serde_json::json!({ "after": narrate("after") }),
        serde_json::json!({
            "before": narrate("before"),
            "during": narrate("during")
        }),
        serde_json::json!({
            "before": narrate("before"),
            "after": narrate("after")
        }),
    ];
    let mut step = self_contained_step();
    step["actions"] = serde_json::Value::Array(
        combinations
            .into_iter()
            .enumerate()
            .map(|(index, narration)| {
                serde_json::json!({
                    "type": "CreateFile",
                    "path": format!("phase-{index}.txt"),
                    "narration": narration
                })
            })
            .collect(),
    );
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/phases.ogg");

    let course = load_package(package.path()).unwrap();
    let actions = &course.playback_steps()[0].actions;

    assert!(actions[0].narration.before.is_some());
    assert!(actions[0].narration.during.is_none());
    assert!(actions[0].narration.after.is_none());
    assert!(actions[1].narration.before.is_none());
    assert!(actions[1].narration.during.is_some());
    assert!(actions[1].narration.after.is_none());
    assert!(actions[2].narration.before.is_none());
    assert!(actions[2].narration.during.is_none());
    assert!(actions[2].narration.after.is_some());
    assert!(actions[3].narration.before.is_some());
    assert!(actions[3].narration.during.is_some());
    assert!(actions[3].narration.after.is_none());
    assert!(actions[4].narration.before.is_some());
    assert!(actions[4].narration.during.is_none());
    assert!(actions[4].narration.after.is_some());
    assert!(
        serde_json::to_value(&actions[0].action)
            .unwrap()
            .get("narration")
            .is_none()
    );
    assert_eq!(
        course.audio_asset_paths(),
        vec![PathBuf::from("assets/narration/phases.ogg")]
    );
}

#[test]
fn action_narration_rejects_during_with_after() {
    for narration in [
        serde_json::json!({
            "during": narrate("during"),
            "after": narrate("after")
        }),
        serde_json::json!({
            "before": narrate("before"),
            "during": narrate("during"),
            "after": narrate("after")
        }),
    ] {
        let mut step = self_contained_step();
        step["actions"][0]["narration"] = narration;
        let package = write_test_package("steps/write_hello.json", step);
        add_unit_audio_asset(&package, "narration", "assets/narration/phases.ogg");

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(
            error.contains("`during` and `after` cannot be combined"),
            "{error}"
        );
    }
}

#[test]
fn narration_fixture_exposes_action_narration_phases_without_vtt() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/narration-demo");

    let course = load_package(&root).unwrap();
    let playback = course.playback_steps();

    assert_eq!(playback.len(), 9);
    let before = course
        .action_narration(
            "ch_narration/sec_narration",
            "create_intro",
            0,
            ActionNarrationPhase::Before,
            0,
        )
        .expect("Before narration");
    assert_eq!(before.offset_ms, Some(0));
    assert_eq!(before.duration_ms, 5_872);
    assert!(
        before
            .audio_path
            .as_ref()
            .is_some_and(|path| path.ends_with("assets/intro.ogg"))
    );

    let during = course
        .action_narration(
            "ch_narration/sec_narration",
            "write_edit",
            2,
            ActionNarrationPhase::During,
            0,
        )
        .expect("During narration");
    assert_eq!(during.duration_ms, 8_946);
    assert_eq!(
        during.caption.as_deref(),
        Some("现在开始输入两行代码。请观察代码自动出现，等输入完成后我们再继续。")
    );

    let caption_only = course
        .action_narration(
            "ch_narration/sec_narration",
            "run_check",
            0,
            ActionNarrationPhase::During,
            0,
        )
        .expect("Caption-only narration");
    assert!(caption_only.audio_path.is_none());
    assert_eq!(caption_only.duration_ms, 1_800);

    let after = course
        .action_narration(
            "ch_narration/sec_narration",
            "create_debrief",
            0,
            ActionNarrationPhase::After,
            0,
        )
        .expect("After narration");
    assert_eq!(after.duration_ms, 6_552);

    let delayed = course
        .action_narration(
            "ch_narration/sec_delayed_edit",
            "write_delayed",
            2,
            ActionNarrationPhase::Before,
            0,
        )
        .expect("delayed Before narration");
    assert_eq!(delayed.duration_ms, 2_166);

    let long = course
        .action_narration(
            "ch_narration/sec_long_narration",
            "write_long_first",
            2,
            ActionNarrationPhase::During,
            0,
        )
        .expect("long During narration");
    assert_eq!(long.duration_ms, 5_132);
    assert!(
        long.audio_path
            .as_ref()
            .is_some_and(|path| path.ends_with("assets/long-continuation.ogg"))
    );

    let single_before = course
        .action_narration(
            "ch_narration/sec_single_step_narration",
            "write_single",
            2,
            ActionNarrationPhase::Before,
            0,
        )
        .expect("single Step Before narration");
    assert_eq!(single_before.duration_ms, 2_166);
    let single_during = course
        .action_narration(
            "ch_narration/sec_single_step_narration",
            "write_single",
            2,
            ActionNarrationPhase::During,
            0,
        )
        .expect("single Step During narration");
    assert_eq!(single_during.duration_ms, 8_112);

    assert!(!root.join("assets/intro.vtt").exists());
}
