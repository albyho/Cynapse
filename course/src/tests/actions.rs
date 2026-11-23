use super::*;

fn line_target(line: u32) -> InsertTextTarget {
    InsertTextTarget {
        line: Some(line),
        at: None,
        placement: None,
        eof: false,
    }
}

#[test]
fn anchor_rejects_invalid_field_combinations() {
    for value in [
        serde_json::json!({"line": 1}),
        serde_json::json!({"line": 1, "at": 0}),
        serde_json::json!({"line": 1, "at": 0, "placement": "after"}),
        serde_json::json!({"at": {"text": "old"}}),
        serde_json::json!({"line": 2, "at": {"text": "old", "index": 1}, "placement": "before"}),
        serde_json::json!({"eof": true}),
    ] {
        let anchor: Anchor = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&anchor).unwrap(), value);
    }

    for value in [
        serde_json::json!({}),
        serde_json::json!({"line": 0}),
        serde_json::json!({"at": 0}),
        serde_json::json!({"at": {"text": ""}}),
        serde_json::json!({"line": 1, "at": {"text": "old", "index": null}}),
        serde_json::json!({"line": 1, "at": {"text": "old", "extra": 1}}),
        serde_json::json!({"line": 1, "at": {"text": "old", "placement": "before"}}),
        serde_json::json!({"line": 1, "at": "old"}),
        serde_json::json!({"line": 1, "index": 0}),
        serde_json::json!({"line": 1, "at": -1}),
        serde_json::json!({"line": 1, "at": 1.5}),
        serde_json::json!({"line": 1, "at": null}),
        serde_json::json!({"line": null}),
        serde_json::json!({"line": 1, "placement": null}),
        serde_json::json!({"line": 1, "placement": "center"}),
        serde_json::json!({"eof": true, "line": 1}),
        serde_json::json!({"eof": true, "at": 0}),
        serde_json::json!({"eof": true, "placement": "before"}),
        serde_json::json!({"eof": null}),
        serde_json::json!({"kind": "line", "line": 1}),
        serde_json::json!({"line": 1, "at": {"text": "old"}, "index": 0}),
        serde_json::json!({"at": {"text": "old"}, "eof": true}),
    ] {
        assert!(
            serde_json::from_value::<Anchor>(value.clone()).is_err(),
            "{value}"
        );
    }
}

#[test]
fn wait_action_uses_a_positive_duration_without_an_app_adapter() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 3000
    }]);
    let package = write_test_package("steps/write_hello.json", step);
    let course = load_package(package.path()).unwrap();

    let Action::Wait(action) = &course.actions()[0] else {
        panic!("expected Wait");
    };
    assert_eq!(action.duration_ms, 3000);
    assert!(course.actions()[0].is_client_local());
    assert!(!course.unit_requires_app_adapter("ch/main"));
}

#[test]
fn wait_action_rejects_a_zero_duration() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 0
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(
        error.contains("Wait duration_ms must be greater than zero"),
        "{error}"
    );
}

#[test]
fn wait_action_rejects_a_duration_over_the_adapter_deadline() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 1_800_001
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(
        error.contains("Wait effective duration must not exceed 1800000 ms"),
        "{error}"
    );
}

#[test]
fn terminal_command_rejects_a_timeout_over_the_adapter_deadline() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "TerminalCommand",
        "session_key": "main",
        "cwd": ".",
        "command": "echo ok",
        "timeout_ms": 1_800_001
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(
        error.contains("TerminalCommand timeout_ms must not exceed 1800000 ms"),
        "{error}"
    );
}

#[test]
fn wait_action_rejects_before_or_after_narration() {
    for phase in ["before", "after"] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([{
            "type": "Wait",
            "duration_ms": 3000,
            "narration": {
                phase: {
                    "clips": [{
                        "duration_ms": 1000,
                        "caption": "Use Present for explanation"
                    }]
                }
            }
        }]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();
        assert!(
            error.contains("Wait may only carry a `during` clip"),
            "{error}"
        );
    }
}

#[test]
fn wait_action_rejects_during_clip_with_declared_duration() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 3000,
        "narration": {
            "during": {
                "clips": [{
                    "duration_ms": 1000,
                    "caption": "请观察执行结果"
                }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(error.contains("must not declare `duration_ms`"), "{error}");
}

#[test]
fn wait_action_accepts_during_audio_clip_and_uses_the_longer_clip_duration() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 3000,
        "narration": {
            "during": {
                "asset_id": "narration",
                "clips": [{ "offset_ms": 0, "duration_ms": 1000, "caption": "观察结果" }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/during.ogg");

    let course = load_package(package.path()).unwrap();
    let playback_action = &course.playback_steps()[0].actions[0];
    let Action::Wait(wait) = &playback_action.action else {
        panic!("expected Wait");
    };
    assert_eq!(wait.duration_ms, 3000);
    let during = playback_action
        .narration
        .during
        .as_ref()
        .expect("Wait during audio present");
    assert_eq!(during.clips.len(), 1);
    assert!(during.clips[0].audio_path.is_some());
    assert_eq!(during.clips[0].offset_ms, Some(0));
    assert_eq!(during.clips[0].duration_ms, 1000);
    // A shorter audio clip does not shorten the authored Wait.
    assert_eq!(playback_action.effective_wait_duration_ms(), Some(3000));
}

#[test]
fn wait_audio_longer_than_the_wait_action_extends_the_effective_duration() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 500,
        "narration": {
            "during": {
                "asset_id": "narration",
                "clips": [{
                    "offset_ms": 200,
                    "duration_ms": 1500,
                    "caption": "延长的观察提示"
                }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/during.ogg");

    let course = load_package(package.path()).unwrap();
    let playback_action = &course.playback_steps()[0].actions[0];

    assert_eq!(
        playback_action.effective_wait_duration_ms(),
        Some(1500),
        "the audio duration extends the Wait; offset_ms must not change it"
    );
}

#[test]
fn wait_action_rejects_a_malformed_during_audio_clip() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 3000,
        "narration": {
            "during": {
                "asset_id": "narration",
                "clips": [{ "offset_ms": 0, "caption": "缺少时长" }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/during.ogg");

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(error.contains("offset_ms and duration_ms"), "{error}");
}

#[test]
fn wait_action_rejects_audio_only_during_clip() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 3000,
        "narration": {
            "during": {
                "asset_id": "narration",
                "clips": [{ "offset_ms": 0, "duration_ms": 1000 }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);
    add_unit_audio_asset(&package, "narration", "assets/narration/during.ogg");

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(error.contains("must declare a caption"), "{error}");
}

#[test]
fn wait_action_rejects_multiple_during_clips() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 3000,
        "narration": {
            "during": {
                "clips": [
                    {"caption": "first"},
                    {"caption": "second"}
                ]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();
    assert!(error.contains("exactly one"), "{error}");
}

#[test]
fn wait_action_accepts_during_caption_and_resolves_duration() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "Wait",
        "duration_ms": 3000,
        "narration": {
            "during": {
                "clips": [{ "caption": "请观察执行结果" }]
            }
        }
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let course = load_package(package.path()).unwrap();
    let playback_action = &course.playback_steps()[0].actions[0];
    let Action::Wait(_) = &playback_action.action else {
        panic!("expected Wait");
    };
    let during = playback_action
        .narration
        .during
        .as_ref()
        .expect("Wait during caption present");
    assert_eq!(during.clips.len(), 1);
    // The clip omits `duration_ms` in the course JSON; the loader backfills it
    // from the Wait action's own `duration_ms`.
    assert_eq!(during.clips[0].duration_ms, 3000);
    assert_eq!(during.clips[0].caption.as_deref(), Some("请观察执行结果"));
}

#[test]
fn rust_hello_fixture_uses_explicit_file_and_insert_actions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rust-hello");
    let course = load_package(&root).unwrap();

    assert_eq!(course.manifest.id, "rust_hello_world");

    let actions = course.actions();
    assert_eq!(actions.len(), 5);
    assert!(matches!(actions[0], Action::CreateFile(_)));
    assert!(matches!(actions[1], Action::OpenFile(_)));
    match &actions[2] {
        Action::InsertText(action) => {
            assert_eq!(action.path, "src/main.rs");
            assert_eq!(action.target, line_target(1));
            assert_eq!(action.speed_ms, Some(50));
            assert_eq!(action.text, "fn main() {\n");
        }
        _ => panic!("expected InsertText"),
    }
    assert!(matches!(actions[3], Action::OpenFile(_)));
    match &actions[4] {
        Action::InsertText(action) => {
            assert_eq!(action.target, line_target(2));
            assert!(action.text.contains("Hello, world!"));
        }
        _ => panic!("expected InsertText"),
    }
}

#[test]
fn legacy_type_code_action_is_rejected() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "TYPE_CODE",
        "target_path": "src/main.rs",
        "line": 1,
        "speed_ms": 50,
        "text": "fn main() {}\n"
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("unknown variant `TYPE_CODE`"), "{error}");
}

#[test]
fn legacy_replace_text_shape_is_rejected() {
    let legacy_actions = [
        serde_json::json!({
            "type": "ReplaceText",
            "target_path": "src/main.rs",
            "target": {"kind": "literal", "text": "old"},
            "text": "new"
        }),
        serde_json::json!({
            "type": "ReplaceText",
            "path": "src/main.rs",
            "selector": {"kind": "literal", "text": "old"},
            "replacement": "new"
        }),
    ];

    for action in legacy_actions {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([
            {
                "type": "OpenFile",
                "path": "src/main.rs"
            },
            action
        ]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains("unknown field"), "{error}");
    }
}

#[test]
fn insert_text_loads_opened_existing_file_and_package_text() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "CreateFile",
            "path": "src/main.rs"
        },
        {
            "type": "OpenFile",
            "path": "src/main.rs",
            "source": "repo_inference",
            "confidence": 0.9,
            "evidence": ["bundle_diff"]
        },
        {
            "type": "InsertText",
            "path": "src/main.rs",
            "target": {"line": 1},
            "text_path": "snippets/main.rs",
            "target_hold_ms": 125,
            "source": "video_inference",
            "confidence": 0.8,
            "evidence": ["ocr", "transcript"]
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);
    std::fs::create_dir_all(package.path().join("snippets")).unwrap();
    std::fs::write(package.path().join("snippets/main.rs"), "fn main() {\n").unwrap();

    let course = load_package(package.path()).unwrap();
    let actions = course.actions();

    assert_eq!(
        serde_json::to_value(&actions[1]).unwrap(),
        serde_json::json!({
            "type": "OpenFile",
            "path": "src/main.rs"
        })
    );
    let wire = serde_json::to_value(&actions[2]).unwrap();
    assert_eq!(wire["type"], "InsertText");
    assert_eq!(wire["path"], "src/main.rs");
    assert_eq!(
        wire["target"],
        serde_json::json!({
          "line": 1
        })
    );
    assert_eq!(wire["text"], "fn main() {\n");
    assert_eq!(wire["target_hold_ms"], 125);
    assert!(wire.get("text_path").is_none());
    assert_eq!(
        course.external_text_paths(),
        vec![PathBuf::from("snippets/main.rs")]
    );
}

#[test]
fn text_path_loads_as_an_external_text_snippet() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "CreateFile",
            "path": "src/main.rs"
        },
        {
            "type": "OpenFile",
            "path": "src/main.rs"
        },
        {
            "type": "InsertText",
            "path": "src/main.rs",
            "target": {"line": 1},
            "text_path": "snippets/main.rs"
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);
    std::fs::create_dir_all(package.path().join("snippets")).unwrap();
    std::fs::write(package.path().join("snippets/main.rs"), "fn main() {\n").unwrap();

    let course = load_package(package.path()).unwrap();
    let actions = course.actions();
    let Action::InsertText(action) = &actions[2] else {
        panic!("expected InsertText");
    };

    assert_eq!(action.text, "fn main() {\n");
    assert_eq!(
        course.external_text_paths(),
        vec![PathBuf::from("snippets/main.rs")]
    );
}

#[test]
fn insert_text_requires_open_file_for_the_same_path_in_the_step() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "OpenFile",
            "path": "src/other.rs"
        },
        {
            "type": "InsertText",
            "path": "src/main.rs",
            "target": {"line": 1},
            "text": "fn main() {}\n"
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("must be preceded by OpenFile for the same path"),
        "{error}"
    );
}

#[test]
fn close_file_resets_open_file_tracking() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        { "type": "OpenFile", "path": "src/main.rs" },
        { "type": "CloseFile", "path": "src/main.rs" },
        {
            "type": "InsertText",
            "path": "src/main.rs",
            "target": {"line": 1},
            "text": "fn main() {}\n"
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("must be preceded by OpenFile for the same path"),
        "{error}"
    );
}

#[test]
fn close_actions_parse_paths_and_ignore_inference_metadata() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        { "type": "OpenFile", "path": "src/main.rs" },
        {
            "type": "CloseFile",
            "path": "src/main.rs",
            "source": "inferred",
            "confidence": 0.9,
            "evidence": ["trace"]
        },
        { "type": "ClosePreview", "path": "README.md" }
    ]);
    let package = write_test_package("steps/write_hello.json", step);

    let course = load_package(package.path()).unwrap();
    let actions = course.actions();

    let Action::CloseFile(close_file) = &actions[1] else {
        panic!("expected CloseFile");
    };
    assert_eq!(close_file.path, "src/main.rs");
    let Action::ClosePreview(close_preview) = &actions[2] else {
        panic!("expected ClosePreview");
    };
    assert_eq!(close_preview.path, "README.md");
}

#[test]
fn close_actions_reject_empty_paths() {
    for (action_type, path) in [("CloseFile", " "), ("ClosePreview", "")] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([
            { "type": "OpenFile", "path": "src/main.rs" },
            { "type": action_type, "path": path }
        ]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(action_type), "{error}");
        assert!(error.contains("path must not be empty"), "{error}");
    }
}

#[test]
fn insert_text_rejects_invalid_sources_and_targets() {
    let cases = [
        (
            serde_json::json!({
              "path": "src/main.rs",
              "target": {"line": 1},
              "text": ""
            }),
            "text must not be empty",
        ),
        (
            serde_json::json!({
              "path": "src/main.rs",
              "target": {"line": 1},
              "target_hold_ms": 0,
              "text": "fn main() {}\n"
            }),
            "target_hold_ms must be greater than zero",
        ),
        (
            serde_json::json!({
              "path": "src/main.rs",
              "target": {"at": {"text": ""}, "placement": "before"},
              "text": "// "
            }),
            "Anchor text must not be empty",
        ),
    ];

    for (mut insert, expected_error) in cases {
        insert["type"] = serde_json::json!("InsertText");
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([
            {
                "type": "OpenFile",
                "path": "src/main.rs"
            },
            insert
        ]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(expected_error), "{error}");
    }
}

#[test]
fn delete_and_replace_load_the_shared_target_model() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "CreateFile",
            "path": "src/main.rs"
        },
        {
            "type": "OpenFile",
            "path": "src/main.rs"
        },
        {
            "type": "DeleteText",
            "path": "src/main.rs",
            "target": {
                "start": {"at": {"text": "temporary", "index": 0}},
                "end": {"at": {"text": "temporary", "index": 0}},
                "expected_text": "temporary"
            },
            "direction": "delete",
            "target_hold_ms": 180
        },
        {
            "type": "DeleteText",
            "path": "src/main.rs",
            "target": {
                "cursor": {
                    "line": 2,
                    "column": 8,
                    "count": 2
                },
                "expected_text": "ab"
            },
            "direction": "backspace",
            "target_hold_ms": 160
        },
        {
            "type": "ReplaceText",
            "path": "src/main.rs",
            "target": {
                "start": {"line": 2},
                "end": {"line": 2},
                "expected_text": "    let answer = 41;"
            },
            "text_path": "snippets/replacement.rs",
            "target_hold_ms": 220
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);
    std::fs::create_dir_all(package.path().join("snippets")).unwrap();
    std::fs::write(
        package.path().join("snippets/replacement.rs"),
        "    let answer = 42;",
    )
    .unwrap();

    let course = load_package(package.path()).unwrap();
    let actions = course.actions();

    let delete = serde_json::to_value(&actions[2]).unwrap();
    assert_eq!(delete["type"], "DeleteText");
    assert_eq!(delete["path"], "src/main.rs");
    assert_eq!(delete["target"]["start"]["at"]["text"], "temporary");
    assert_eq!(delete["target"]["end"]["at"]["text"], "temporary");
    assert_eq!(delete["direction"], "delete");
    assert_eq!(delete["target_hold_ms"], 180);

    let cursor_delete = serde_json::to_value(&actions[3]).unwrap();
    assert_eq!(cursor_delete["type"], "DeleteText");
    assert_eq!(cursor_delete["target"]["cursor"]["count"], 2);
    assert_eq!(cursor_delete["target"]["expected_text"], "ab");
    assert_eq!(cursor_delete["direction"], "backspace");
    assert_eq!(cursor_delete["target_hold_ms"], 160);

    let replace = serde_json::to_value(&actions[4]).unwrap();
    assert_eq!(replace["type"], "ReplaceText");
    assert_eq!(replace["path"], "src/main.rs");
    assert_eq!(replace["target"]["start"]["line"], 2);
    assert_eq!(replace["target"]["end"]["line"], 2);
    assert_eq!(replace["text"], "    let answer = 42;");
    assert_eq!(replace["target_hold_ms"], 220);
    assert!(replace.get("text_path").is_none());
    assert!(replace.get("selector").is_none());
    assert!(replace.get("replacement").is_none());
    assert_eq!(
        course.external_text_paths(),
        vec![PathBuf::from("snippets/replacement.rs")]
    );
}

#[test]
fn delete_and_replace_reject_invalid_targets_and_speeds() {
    let cases = [
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "cursor": {
                        "line": 1,
                        "column": 0,
                        "count": 0
                    },
                    "expected_text": "x"
                },
                "direction": "backspace"
            }),
            "line and count must be at least 1",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "cursor": {
                        "line": 1,
                        "column": 0,
                        "count": 1
                    },
                    "expected_text": "x"
                }
            }),
            "cursor target requires `direction`",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "cursor": {
                        "line": 1,
                        "column": 0,
                        "direction": "backspace",
                        "count": 1
                    },
                    "expected_text": "x"
                },
                "direction": "backspace"
            }),
            "unknown field `direction`",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "start": {"at": {"text": "temporary"}},
                    "end": {"at": {"text": "temporary"}},
                    "expected_text": "temporary"
                },
                "target_hold_ms": 0
            }),
            "target_hold_ms must be greater than zero",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 1},
                    "end": {"line": 1},
                    "expected_text": "old"
                },
                "text": ""
            }),
            "text must not be empty",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 1},
                    "end": {"line": 1},
                    "expected_text": "old"
                },
                "text": "new",
                "speed_ms": 0,
                "edit_mode": "delete_then_type"
            }),
            "edit_mode is only valid with a positive speed_ms",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 1},
                    "end": {"line": 1},
                    "expected_text": "old"
                },
                "text": "new",
                "target_hold_ms": 0
            }),
            "target_hold_ms must be greater than zero",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "cursor": {
                        "line": 1,
                        "column": 0,
                        "count": 1
                    }
                },
                "text": "new"
            }),
            "unknown field `cursor`",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 1, "text": "old"},
                    "end": {"line": 1},
                    "expected_text": "old"
                },
                "text": "new"
            }),
            "unknown Anchor field `text`",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"at": {"text": "old"}, "placement": "after"},
                    "end": {"at": {"text": "old"}, "placement": "before"},
                    "expected_text": "old"
                },
                "text": "new"
            }),
            "text anchors must form a non-empty range",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 2},
                    "end": {"line": 1},
                    "expected_text": "old"
                },
                "text": "new"
            }),
            "non-empty forward range",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"at": {"text": "old"}},
                    "end": {"at": {"text": "old"}},
                    "expected_text": ""
                },
                "text": "new"
            }),
            "expected_text must not be empty",
            true,
        ),
        (
            serde_json::json!({
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 1, "at": null},
                    "end": {"line": 2},
                    "expected_text": "old"
                },
                "text": "new"
            }),
            "Anchor `at` must not be null",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 1},
                    "end": {"line": 2},
                    "expected_text": "old",
                    "include_end_line_break": true
                }
            }),
            "unknown DeleteText target field `include_end_line_break`",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "start": {"eof": true},
                    "end": {"line": 2},
                    "expected_text": "old"
                }
            }),
            "start anchor must not be eof",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "start": {"eof": true, "line": 1},
                    "end": {"line": 2},
                    "expected_text": "old"
                }
            }),
            "eof Anchor cannot include other fields",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "start": {"at": 0},
                    "end": {"line": 2},
                    "expected_text": "old"
                }
            }),
            "column Anchor requires `line`",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "start": {"line": 0},
                    "end": {"line": 2},
                    "expected_text": "old"
                }
            }),
            "Anchor line must be at least 1",
            true,
        ),
        (
            serde_json::json!({
                "type": "DeleteText",
                "path": "src/main.rs",
                "target": {
                    "start": {"at": {"text": ""}},
                    "end": {"line": 2},
                    "expected_text": "old"
                }
            }),
            "Anchor text must not be empty",
            true,
        ),
    ];

    for (action, expected_error, include_open) in cases {
        let mut step = self_contained_step();
        let mut actions = Vec::new();
        if include_open {
            actions.push(serde_json::json!({
                "type": "OpenFile",
                "path": "src/main.rs"
            }));
        }
        actions.push(action);
        step["actions"] = serde_json::Value::Array(actions);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(expected_error), "{error}");
    }
}

#[test]
fn text_range_anchor_placements_are_all_valid() {
    for (start_placement, end_placement) in [
        ("before", "after"),
        ("after", "before"),
        ("before", "before"),
        ("after", "after"),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([
            {
                "type": "OpenFile",
                "path": "src/main.rs"
            },
            {
                "type": "ReplaceText",
                "path": "src/main.rs",
                "target": {
                    "start": {"at": {"text": "old"}, "placement": start_placement},
                    "end": {"at": {"text": "new"}, "placement": end_placement},
                    "expected_text": "old\nnew"
                },
                "text": "changed"
            }
        ]);
        let package = write_test_package("steps/write_hello.json", step);

        load_package(package.path()).unwrap();
    }
}

#[test]
fn edit_range_accepts_an_eof_end() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "OpenFile",
            "path": "src/main.rs"
        },
        {
            "type": "ReplaceText",
            "path": "src/main.rs",
            "target": {
                "start": {"line": 1},
                "end": {"eof": true},
                "expected_text": "old"
            },
            "text": "new"
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);

    load_package(package.path()).unwrap();
}

#[test]
fn delete_and_replace_require_open_file_for_the_same_path() {
    let actions = [
        serde_json::json!({
            "type": "DeleteText",
            "path": "src/main.rs",
            "target": {
                "start": {"at": {"text": "temporary"}},
                "end": {"at": {"text": "temporary"}},
                "expected_text": "temporary"
            }
        }),
        serde_json::json!({
            "type": "ReplaceText",
            "path": "src/main.rs",
            "target": {
                "start": {"at": {"text": "old"}},
                "end": {"at": {"text": "old"}},
                "expected_text": "old"
            },
            "text": "new"
        }),
    ];

    for action in actions {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([
            {
                "type": "OpenFile",
                "path": "src/other.rs"
            },
            action
        ]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(
            error.contains("must be preceded by OpenFile for the same path"),
            "{error}"
        );
    }
}

#[test]
fn component_test_courses_use_only_explicit_code_editing_actions() {
    fn uses_type_code(directory: &Path) -> bool {
        std::fs::read_dir(directory).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                uses_type_code(&path)
            } else {
                path.extension().and_then(|extension| extension.to_str()) == Some("json")
                    && std::fs::read_to_string(path)
                        .unwrap()
                        .contains("\"type\": \"TYPE_CODE\"")
            }
        })
    }

    fn collect_packages(directory: &Path, packages: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.join("manifest.json").is_file() {
                    packages.push(path);
                } else {
                    collect_packages(&path, packages);
                }
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
    let mut packages = Vec::new();
    collect_packages(&root, &mut packages);
    packages.sort();

    assert!(!packages.is_empty());
    for package in packages {
        load_package(&package).unwrap();
        assert!(
            !uses_type_code(&package),
            "component test package `{}` still uses TYPE_CODE",
            package.display()
        );
    }
}

#[test]
fn obsolete_action_fields_are_rejected() {
    for (field, value) in [
        ("time_ms", serde_json::json!(0)),
        ("mode", serde_json::json!("presentation")),
        ("caption", serde_json::json!("Unused caption")),
        ("after_segments", serde_json::json!(["intro"])),
        ("segment_id", serde_json::json!("legacy")),
        ("spans_steps", serde_json::json!(["step"])),
        ("start_with_step", serde_json::json!("step")),
        ("start_after_step", serde_json::json!("step")),
        ("hold_until_step", serde_json::json!("step")),
    ] {
        let mut step = self_contained_step();
        step[field] = value;
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(
            error.contains(field),
            "expected `{field}` to be rejected, got: {error}"
        );
    }
}

#[test]
fn steps_must_contain_at_least_one_action() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([]);

    let package = write_test_package("steps/write_hello.json", step);
    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("step `write_hello` must contain at least one Action"),
        "{error}"
    );
}

#[test]
fn filesystem_fixture_flattens_actions_grouped_by_step() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/filesystem-actions");
    let course = load_package(&root).unwrap();
    let actions = course.actions();

    assert_eq!(actions.len(), 7);
    assert!(matches!(actions[0], Action::CreateDirectory(_)));
    assert!(matches!(actions[1], Action::CreateFile(_)));
    assert!(matches!(actions[2], Action::OpenFile(_)));
    assert!(matches!(actions[3], Action::InsertText(_)));
    assert!(matches!(actions[4], Action::DeleteFile(_)));
    assert!(matches!(actions[5], Action::DeleteDirectory(_)));
    assert!(matches!(actions[6], Action::DeleteDirectory(_)));
}

#[test]
fn rust_40_line_fixture_loads_one_complete_insert_action() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rust-40-lines");
    let course = load_package(&root).unwrap();
    let actions = course.actions();

    assert_eq!(actions.len(), 3);
    let Action::InsertText(action) = &actions[2] else {
        panic!("expected InsertText");
    };
    assert_eq!(action.path, "src/main.rs");
    assert_eq!(action.target, line_target(1));
    assert_eq!(action.speed_ms, Some(30));
    assert_eq!(action.text.lines().count(), 40);
    assert_eq!(
        action.text.lines().next(),
        Some("// A 40-line Rust baseline used by Cynapse.")
    );
    assert_eq!(action.text.lines().last(), Some("}"));
}

#[test]
fn removed_code_action_names_report_the_text_replacement() {
    for (old, new) in [
        ("INSERT_CODE", "InsertText"),
        ("REPLACE_CODE", "ReplaceText"),
        ("DELETE_CODE", "DeleteText"),
        ("REVEAL_CODE", "RevealText"),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([{ "type": old }]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(&format!("`{old}`")), "{error}");
        assert!(error.contains(&format!("`{new}`")), "{error}");
        assert!(error.contains("rename"), "{error}");
    }
}

#[test]
fn removed_code_fields_report_the_text_replacement() {
    for (old, new) in [("code", "text"), ("code_path", "text_path")] {
        let mut action = serde_json::json!({
          "type": "InsertText",
          "path": "src/main.rs",
          "target": {
            "eof": true
          }
        });
        action[old] = serde_json::json!("fn main() {}\n");
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([
            { "type": "CreateFile", "path": "src/main.rs" },
            { "type": "OpenFile", "path": "src/main.rs" },
            action
        ]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(&format!("`{old}`")), "{error}");
        assert!(error.contains(&format!("`{new}`")), "{error}");
        assert!(error.contains("rename"), "{error}");
    }
}

#[test]
fn rejects_write_and_terminal_paths_that_leave_the_unit_workspace() {
    let cases = [
        (
            serde_json::json!([
                { "type": "OpenFile", "path": "../outside.txt" },
                {
                    "type": "InsertText",
                    "path": "../outside.txt",
                    "target": {"line": 1},
                    "text": "x"
                }
            ]),
            "InsertText path",
        ),
        (
            serde_json::json!([
                { "type": "TerminalEnsure", "session_key": "main", "cwd": "../outside" }
            ]),
            "TerminalEnsure cwd",
        ),
        (
            serde_json::json!([
                {
                    "type": "TerminalCommand",
                    "session_key": "main",
                    "cwd": ".",
                    "command": "echo ok",
                    "completion_marker": "../outside"
                }
            ]),
            "TerminalCommand completion_marker",
        ),
    ];

    for (actions, expected) in cases {
        let mut step = self_contained_step();
        step["actions"] = actions;
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();
        assert!(
            error.contains(expected) && error.contains("must stay inside the Unit Workspace"),
            "{error}"
        );
    }
}

#[test]
fn delete_directory_cannot_target_the_unit_workspace_root() {
    for path in [".", "./"] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([{
            "type": "DeleteDirectory",
            "path": path,
            "recursive": true
        }]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains("DeleteDirectory path"), "{error}");
        assert!(error.contains("Unit Workspace root"), "{error}");
    }
}

#[test]
fn write_actions_reject_an_empty_path() {
    for (action_type, error_field) in [
        ("CreateDirectory", "CreateDirectory path"),
        ("CreateFile", "CreateFile path"),
        ("DeleteFile", "DeleteFile path"),
        ("DeleteDirectory", "DeleteDirectory path"),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([{
            "type": action_type,
            "path": ""
        }]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(error_field), "{error}");
        assert!(error.contains("must not be empty"), "{error}");
    }
}

#[test]
fn terminal_actions_reject_empty_required_fields() {
    for (action, field) in [
        (
            serde_json::json!({
                "type": "TerminalEnsure",
                "session_key": "",
                "cwd": "."
            }),
            "TerminalEnsure session_key",
        ),
        (
            serde_json::json!({
                "type": "TerminalCommand",
                "session_key": "",
                "cwd": ".",
                "command": "echo ok"
            }),
            "TerminalCommand session_key",
        ),
        (
            serde_json::json!({
                "type": "TerminalCommand",
                "session_key": "main",
                "cwd": ".",
                "command": ""
            }),
            "TerminalCommand command",
        ),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([action]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(field), "{error}");
        assert!(error.contains("must not be empty"), "{error}");
    }
}

#[test]
fn terminal_command_rejects_invalid_execution_contract() {
    for (action, expected) in [
        (
            serde_json::json!({
                "type": "TerminalCommand",
                "session_key": "main",
                "cwd": ".",
                "command": "echo ok",
                "timeout_ms": 0
            }),
            "timeout_ms must be greater than zero",
        ),
        (
            serde_json::json!({
                "type": "TerminalCommand",
                "session_key": "main",
                "cwd": ".",
                "command": "echo ok",
                "accepted_exit_codes": []
            }),
            "accepted_exit_codes must not be empty",
        ),
        (
            serde_json::json!({
                "type": "TerminalCommand",
                "session_key": "main",
                "cwd": ".",
                "command": "echo ok",
                "expected_output": [""]
            }),
            "expected_output must not contain empty strings",
        ),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([action]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn jupyterlab_notebook_actions_parse_with_defaults() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        { "type": "JupyterLabOpenNotebook", "path": "lesson.ipynb" },
        {
            "type": "JupyterLabCellSelect",
            "path": "lesson.ipynb",
            "cell_id": "cell-1",
            "align": "center"
        },
        {
            "type": "JupyterLabCellRun",
            "path": "lesson.ipynb",
            "from_cell_id": "cell-1",
            "to_cell_id": "cell-1"
        },
        { "type": "JupyterLabRestartKernel", "path": "lesson.ipynb" },
        { "type": "JupyterLabInterruptKernel", "path": "lesson.ipynb" },
        { "type": "JupyterLabOpenPreview", "path": "INTRO.md" },
        {
            "type": "JupyterLabCellInsertText",
            "path": "lesson.ipynb",
            "cell_id": "cell-1",
            "target": {"eof": true},
            "text": "x = 1\n",
            "speed_ms": 7
        },
        {
            "type": "JupyterLabCellInsert",
            "path": "lesson.ipynb",
            "cell_id": "cynapse_demo_1",
            "after_cell_id": "cell-1",
            "cell_type": "markdown",
            "source": "## Demo"
        },
        {
            "type": "JupyterLabCellDelete",
            "path": "lesson.ipynb",
            "cell_id": "cynapse_demo_1"
        },
        { "type": "JupyterLabCloseDocument", "path": "lesson.ipynb" }
    ]);
    let package = write_test_package("steps/write_hello.json", step);
    let course = load_package(package.path()).unwrap();
    let actions = course.actions();

    let Action::JupyterLabOpenNotebook(open) = &actions[0] else {
        panic!("expected JupyterLabOpenNotebook");
    };
    assert_eq!(open.path, "lesson.ipynb");

    let Action::JupyterLabCellSelect(select) = &actions[1] else {
        panic!("expected JupyterLabCellSelect");
    };
    assert_eq!(select.path, "lesson.ipynb");
    assert_eq!(select.cell_id, "cell-1");
    assert!(select.scroll);
    assert_eq!(select.align, Some(JupyterLabCellScrollAlign::Center));

    let Action::JupyterLabCellRun(run) = &actions[2] else {
        panic!("expected JupyterLabCellRun");
    };
    assert_eq!(run.from_cell_id.as_deref(), Some("cell-1"));
    assert_eq!(run.to_cell_id.as_deref(), Some("cell-1"));
    assert_eq!(run.timeout_ms, 300_000);

    let Action::JupyterLabRestartKernel(restart) = &actions[3] else {
        panic!("expected JupyterLabRestartKernel");
    };
    assert_eq!(restart.path, "lesson.ipynb");

    let Action::JupyterLabInterruptKernel(interrupt) = &actions[4] else {
        panic!("expected JupyterLabInterruptKernel");
    };
    assert_eq!(interrupt.path, "lesson.ipynb");

    let Action::JupyterLabOpenPreview(preview) = &actions[5] else {
        panic!("expected JupyterLabOpenPreview");
    };
    assert_eq!(preview.path, "INTRO.md");

    let Action::JupyterLabCellInsertText(type_source) = &actions[6] else {
        panic!("expected JupyterLabCellInsertText");
    };
    assert_eq!(type_source.cell_id, "cell-1");
    assert_eq!(type_source.text, "x = 1\n");
    assert_eq!(type_source.speed_ms, Some(7));

    let Action::JupyterLabCellInsert(insert) = &actions[7] else {
        panic!("expected JupyterLabCellInsert");
    };
    assert_eq!(insert.cell_id, "cynapse_demo_1");
    assert_eq!(insert.after_cell_id.as_deref(), Some("cell-1"));
    assert_eq!(insert.cell_type, JupyterLabCellType::Markdown);
    assert_eq!(insert.source, "## Demo");

    let Action::JupyterLabCellDelete(delete) = &actions[8] else {
        panic!("expected JupyterLabCellDelete");
    };
    assert_eq!(delete.cell_id, "cynapse_demo_1");

    let Action::JupyterLabCloseDocument(close) = &actions[9] else {
        panic!("expected JupyterLabCloseDocument");
    };
    assert_eq!(close.path, "lesson.ipynb");
    assert!(actions.iter().all(|action| !action.is_client_local()));
}

#[test]
fn jupyterlab_run_cell_accepts_range_and_all() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "JupyterLabCellRun",
            "path": "lesson.ipynb",
            "from_cell_id": "setup",
            "to_cell_id": "cell-1"
        },
        { "type": "JupyterLabCellRun", "path": "lesson.ipynb" }
    ]);
    let package = write_test_package("steps/write_hello.json", step);
    let course = load_package(package.path()).unwrap();
    let actions = course.actions();

    let Action::JupyterLabCellRun(range) = &actions[0] else {
        panic!("expected JupyterLabCellRun range");
    };
    assert_eq!(range.from_cell_id.as_deref(), Some("setup"));
    assert_eq!(range.to_cell_id.as_deref(), Some("cell-1"));

    let Action::JupyterLabCellRun(all) = &actions[1] else {
        panic!("expected JupyterLabCellRun all");
    };
    assert!(all.from_cell_id.is_none());
    assert!(all.to_cell_id.is_none());
}

#[test]
fn jupyterlab_cell_move_duplicate_and_before_insert_parse() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "JupyterLabCellInsert",
            "path": "lesson.ipynb",
            "cell_id": "cynapse_before",
            "before_cell_id": "cell-1",
            "cell_type": "markdown",
            "source": "## Before"
        },
        {
            "type": "JupyterLabCellMove",
            "path": "lesson.ipynb",
            "cell_id": "cynapse_before",
            "direction": "down"
        },
        {
            "type": "JupyterLabCellDuplicate",
            "path": "lesson.ipynb",
            "cell_id": "cynapse_before",
            "new_cell_id": "cynapse_before_copy"
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);
    let course = load_package(package.path()).unwrap();

    let Action::JupyterLabCellInsert(insert) = &course.actions()[0] else {
        panic!("expected JupyterLabCellInsert");
    };
    assert_eq!(insert.before_cell_id.as_deref(), Some("cell-1"));
    assert_eq!(insert.after_cell_id, None);

    let Action::JupyterLabCellMove(move_cell) = &course.actions()[1] else {
        panic!("expected JupyterLabCellMove");
    };
    assert_eq!(move_cell.direction, JupyterLabMoveDirection::Down);
    assert_eq!(move_cell.count, 1);

    let Action::JupyterLabCellDuplicate(duplicate) = &course.actions()[2] else {
        panic!("expected JupyterLabCellDuplicate");
    };
    assert_eq!(
        duplicate.new_cell_id.as_deref(),
        Some("cynapse_before_copy")
    );
}

#[test]
fn jupyterlab_insert_cell_allows_an_empty_notebook_first_cell() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        {
            "type": "JupyterLabCellInsert",
            "path": "empty.ipynb",
            "cell_id": "cynapse_first",
            "cell_type": "code",
            "source": ""
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);
    let course = load_package(package.path()).unwrap();

    let Action::JupyterLabCellInsert(insert) = &course.actions()[0] else {
        panic!("expected JupyterLabCellInsert");
    };
    assert_eq!(insert.cell_id, "cynapse_first");
    assert_eq!(insert.after_cell_id, None);
}

#[test]
fn jupyterlab_notebook_actions_reject_empty_ids_and_paths() {
    for (action, expected) in [
        (
            serde_json::json!({ "type": "JupyterLabOpenNotebook", "path": "" }),
            "JupyterLabOpenNotebook path",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellSelect",
                "path": "lesson.ipynb",
                "cell_id": ""
            }),
            "JupyterLabCellSelect cell_id",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellRun",
                "path": "",
                "from_cell_id": "cell-1",
                "to_cell_id": "cell-1"
            }),
            "JupyterLabCellRun path",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellRun",
                "path": "lesson.ipynb",
                "from_cell_id": ""
            }),
            "JupyterLabCellRun from_cell_id",
        ),
        (
            serde_json::json!({ "type": "JupyterLabOpenPreview", "path": "" }),
            "JupyterLabOpenPreview path",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellInsertText",
                "path": "lesson.ipynb",
                "cell_id": "",
                "target": {"eof": true},
                "text": "x"
            }),
            "JupyterLabCellInsertText cell_id",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellInsert",
                "path": "lesson.ipynb",
                "cell_id": "cynapse_demo_1",
                "after_cell_id": "",
                "cell_type": "code",
                "source": ""
            }),
            "JupyterLabCellInsert after_cell_id",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellDelete",
                "path": "lesson.ipynb",
                "cell_id": ""
            }),
            "JupyterLabCellDelete cell_id",
        ),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([action]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(expected), "{error}");
        assert!(error.contains("must not be empty"), "{error}");
    }
}

#[test]
fn jupyterlab_text_edits_reject_invalid_targets() {
    for (action, expected) in [
        (
            serde_json::json!({
                "type": "JupyterLabCellInsertText",
                "path": "lesson.ipynb",
                "cell_id": "cell-1",
                "target": {"line": 0},
                "text": "x"
            }),
            "Anchor line must be at least 1",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellInsertText",
                "path": "lesson.ipynb",
                "cell_id": "cell-1",
                "target": {"line": 0, "at": 0},
                "text": "x"
            }),
            "Anchor line must be at least 1",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellInsertText",
                "path": "lesson.ipynb",
                "cell_id": "cell-1",
                "target": {"at": {"text": ""}, "placement": "before"},
                "text": "x"
            }),
            "Anchor text must not be empty",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellDeleteText",
                "path": "lesson.ipynb",
                "cell_id": "cell-1",
                "target": {
                    "cursor": { "line": 0, "column": 0, "count": 1 },
                    "expected_text": "x"
                }
            }),
            "JupyterLabCellDeleteText cursor line and count must be at least 1",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellDeleteText",
                "path": "lesson.ipynb",
                "cell_id": "cell-1",
                "target": {
                    "start": { "line": 1 },
                    "end": {"line": 1, "at": 1},
                    "expected_text": ""
                }
            }),
            "JupyterLabCellDeleteText expected_text must not be empty",
        ),
        (
            serde_json::json!({
                "type": "JupyterLabCellReplaceText",
                "path": "lesson.ipynb",
                "cell_id": "cell-1",
                "target": {
                    "start": { "line": 1 },
                    "end": {"line": 1, "at": 1},
                    "expected_text": ""
                },
                "text": "x"
            }),
            "JupyterLabCellReplaceText expected_text must not be empty",
        ),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([action]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn jupyterlab_new_actions_reject_non_markdown_preview() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{ "type": "JupyterLabOpenPreview", "path": "INTRO.txt" }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("JupyterLabOpenPreview path must be Markdown"),
        "{error}"
    );
}

#[test]
fn jupyterlab_cell_select_align_requires_scrolling() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "JupyterLabCellSelect",
        "path": "lesson.ipynb",
        "cell_id": "cell-1",
        "scroll": false,
        "align": "center"
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("JupyterLabCellSelect align requires scroll"),
        "{error}"
    );
}

#[test]
fn jupyterlab_cell_select_rejects_an_unknown_align() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([{
        "type": "JupyterLabCellSelect",
        "path": "lesson.ipynb",
        "cell_id": "cell-1",
        "align": "middle"
    }]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("unknown variant `middle`"), "{error}");
}

#[test]
fn jupyterlab_cell_insert_text_accepts_zero_and_null_speed() {
    for (speed, expected) in [
        (serde_json::json!(0), Some(0)),
        (serde_json::Value::Null, None),
    ] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([{
            "type": "JupyterLabCellInsertText",
            "path": "lesson.ipynb",
            "cell_id": "cell-1",
            "target": {"eof": true},
            "text": "x = 1\n",
            "speed_ms": speed
        }]);
        let package = write_test_package("steps/write_hello.json", step);

        let course = load_package(package.path()).unwrap();
        let Action::JupyterLabCellInsertText(action) = &course.actions()[0] else {
            panic!("expected JupyterLabCellInsertText");
        };
        assert_eq!(action.speed_ms, expected);
    }
}

#[test]
fn run_cell_rejects_a_timeout_over_the_adapter_deadline() {
    for timeout in [0, 1_800_001] {
        let mut step = self_contained_step();
        step["actions"] = serde_json::json!([
            { "type": "JupyterLabOpenNotebook", "path": "lesson.ipynb" },
            {
                "type": "JupyterLabCellRun",
                "path": "lesson.ipynb",
                "from_cell_id": "cell-1",
                "to_cell_id": "cell-1",
                "timeout_ms": timeout
            }
        ]);
        let package = write_test_package("steps/write_hello.json", step);

        let error = load_package(package.path()).unwrap_err().to_string();

        assert!(error.contains("JupyterLabCellRun timeout_ms"), "{error}");
    }
}

#[test]
fn jupyterlab_notebook_paths_stay_inside_the_unit_workspace() {
    let mut step = self_contained_step();
    step["actions"] = serde_json::json!([
        { "type": "JupyterLabOpenNotebook", "path": "../outside.ipynb" },
        {
            "type": "JupyterLabCellSelect",
            "path": "../outside.ipynb",
            "cell_id": "cell-1"
        }
    ]);
    let package = write_test_package("steps/write_hello.json", step);

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(error.contains("JupyterLabOpenNotebook path"), "{error}");
    assert!(
        error.contains("must stay inside the Unit Workspace"),
        "{error}"
    );
}

#[test]
fn playback_steps_preserve_node_and_step_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/multi-step-demo");
    let course = load_package(&root).unwrap();
    let steps = course.playback_steps();

    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].parent_id, "ch_multi_step");
    assert_eq!(steps[0].parent_title, "Step Demo");
    assert_eq!(steps[0].step_id, "write_first");
    assert_eq!(steps[0].step_title, "First Step");
    assert_eq!(steps[0].actions.len(), 3);
    assert_eq!(steps[1].step_id, "write_second");
    assert_eq!(steps[1].step_title, "Second Step");
    assert_eq!(steps[1].actions.len(), 2);
}
