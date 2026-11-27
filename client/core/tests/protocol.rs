use cynapse_client_core::protocol::WorkspaceResetPayload;
use cynapse_client_core::{
    AckPayload, AckStatus, Action, Anchor, AnchorAt, CancelPayload, CancelReason, DeleteDirection,
    DeleteTextTarget, Envelope, HelloPayload, JupyterLabCellScrollAlign, MessageType,
    MissingTargetPolicy, PROTOCOL_VERSION, Placement, ReplaceTextEditMode, TextAnchor,
};

#[test]
fn hello_round_trips_with_protocol_version() {
    let envelope = Envelope::hello("test-token");
    let json = envelope.to_json().unwrap();
    let parsed = Envelope::parse(&json).unwrap();

    assert_eq!(parsed.v, PROTOCOL_VERSION);
    assert_eq!(parsed.kind, MessageType::Hello);
    assert_eq!(
        serde_json::from_value::<HelloPayload>(parsed.payload).unwrap(),
        HelloPayload {
            token: "test-token".to_string(),
            role: "adapter".to_string(),
        },
    );
}

#[test]
fn hello_ack_uses_snake_case_message_type() {
    let envelope = Envelope::hello_ack("vscode");
    let json = envelope.to_json().unwrap();

    assert!(json.contains("\"type\":\"hello_ack\""));
    assert_eq!(envelope.v, PROTOCOL_VERSION);
}

#[test]
fn golden_hello_ack_reports_the_jupyterlab_adapter() {
    let envelope = Envelope::parse(include_str!(
        "../../../protocol/test-data/hello_ack_jupyterlab.json"
    ))
    .unwrap();
    let payload =
        serde_json::from_value::<cynapse_client_core::HelloAckPayload>(envelope.payload).unwrap();

    assert_eq!(envelope.kind, MessageType::HelloAck);
    assert_eq!(payload.adapter, "jupyterlab");
}

#[test]
fn ack_error_correlates_with_target_id() {
    let envelope = Envelope::ack_error("hello-id", "token_mismatch", "invalid token");
    let parsed = Envelope::parse(&envelope.to_json().unwrap()).unwrap();
    let payload = serde_json::from_value::<AckPayload>(parsed.payload).unwrap();

    assert_eq!(parsed.id, "hello-id");
    assert_eq!(parsed.kind, MessageType::Ack);
    assert_eq!(payload.status, AckStatus::Error);
    assert_eq!(payload.code.as_deref(), Some("token_mismatch"));
}

#[test]
fn golden_hello_fixture_matches_contract() {
    let envelope = Envelope::parse(include_str!("../../../protocol/test-data/hello.json")).unwrap();
    let payload = serde_json::from_value::<HelloPayload>(envelope.payload).unwrap();

    assert_eq!(envelope.v, PROTOCOL_VERSION);
    assert_eq!(envelope.kind, MessageType::Hello);
    assert_eq!(payload.role, "adapter");
}

#[test]
fn golden_ack_fixture_matches_contract() {
    let envelope =
        Envelope::parse(include_str!("../../../protocol/test-data/ack_error.json")).unwrap();
    let payload = serde_json::from_value::<AckPayload>(envelope.payload).unwrap();

    assert_eq!(envelope.kind, MessageType::Ack);
    assert_eq!(payload.status, AckStatus::Error);
    assert_eq!(payload.code.as_deref(), Some("token_mismatch"));
}

#[test]
fn terminal_ack_preserves_exit_code_and_output() {
    let envelope = Envelope::parse(include_str!(
        "../../../protocol/test-data/ack_terminal.json"
    ))
    .unwrap();
    let payload = serde_json::from_value::<AckPayload>(envelope.payload).unwrap();

    assert_eq!(payload.status, AckStatus::Done);
    assert_eq!(payload.exit_code, Some(0));
    assert_eq!(payload.output.as_deref(), Some("Hello, world!\n"));
    assert_eq!(payload.output_truncated, Some(false));
    assert_eq!(payload.completion_marker_found, Some(true));
}

#[test]
fn interrupted_terminal_ack_preserves_partial_output() {
    let envelope = Envelope::parse(include_str!(
        "../../../protocol/test-data/ack_terminal_interrupted.json"
    ))
    .unwrap();
    let payload = serde_json::from_value::<AckPayload>(envelope.payload).unwrap();

    assert_eq!(payload.status, AckStatus::Cancelled);
    assert_eq!(payload.code.as_deref(), Some("terminal_timeout"));
    assert_eq!(payload.timed_out, Some(true));
    assert_eq!(payload.output.as_deref(), Some("partial compiler output\n"));
    assert_eq!(payload.output_truncated, Some(false));
}

#[test]
fn golden_cancel_fixture_matches_contract() {
    let envelope =
        Envelope::parse(include_str!("../../../protocol/test-data/cancel.json")).unwrap();
    let payload = serde_json::from_value::<CancelPayload>(envelope.payload).unwrap();

    assert_eq!(envelope.kind, MessageType::Cancel);
    assert_eq!(payload.target_id, "6b7e7f32-4aef-4f92-a4c3-8b3219bbd5e1");
    assert_eq!(payload.reason, Some(CancelReason::UserStop));
}

#[test]
fn workspace_reset_round_trips_deleted_paths_and_exclusions() {
    let envelope =
        Envelope::workspace_reset(vec!["src".to_string()], vec!["src/child".to_string()]);
    let parsed = Envelope::parse(&envelope.to_json().unwrap()).unwrap();
    let payload = serde_json::from_value::<WorkspaceResetPayload>(parsed.payload).unwrap();

    assert_eq!(parsed.kind, MessageType::WorkspaceReset);
    assert_eq!(payload.paths, vec!["src".to_string()]);
    assert_eq!(payload.exclude_paths, vec!["src/child".to_string()]);
    assert!(payload.close_tabs);
}

#[test]
fn terminal_cleanup_keeps_editor_tabs_open() {
    let envelope = Envelope::terminal_cleanup(vec!["ch/unit".to_string()], Vec::new());
    let payload =
        serde_json::from_value::<WorkspaceResetPayload>(envelope.payload.clone()).unwrap();

    assert_eq!(envelope.kind, MessageType::WorkspaceReset);
    assert_eq!(payload.paths, vec!["ch/unit".to_string()]);
    assert!(payload.exclude_paths.is_empty());
    assert!(!payload.close_tabs);
}

#[test]
fn golden_action_fixtures_cover_every_primitive() {
    let set_cursor = include_str!("../../../protocol/test-data/action_set_cursor.json");
    let fixtures = [
        include_str!("../../../protocol/test-data/action_present.json"),
        include_str!("../../../protocol/test-data/action_replace_text.json"),
        include_str!("../../../protocol/test-data/action_create_directory.json"),
        include_str!("../../../protocol/test-data/action_create_file.json"),
        include_str!("../../../protocol/test-data/action_delete_file.json"),
        include_str!("../../../protocol/test-data/action_delete_directory.json"),
        include_str!("../../../protocol/test-data/action_terminal_ensure.json"),
        include_str!("../../../protocol/test-data/action_terminal_command.json"),
        include_str!("../../../protocol/test-data/action_terminal_failure.json"),
        include_str!("../../../protocol/test-data/action_open_file.json"),
        include_str!("../../../protocol/test-data/action_open_preview.json"),
        include_str!("../../../protocol/test-data/action_open_file_with_preview.json"),
        include_str!("../../../protocol/test-data/action_reveal_text.json"),
        include_str!("../../../protocol/test-data/action_clear_reveal.json"),
        include_str!("../../../protocol/test-data/action_insert_text.json"),
        include_str!("../../../protocol/test-data/action_delete_text.json"),
        include_str!("../../../protocol/test-data/action_delete_cursor.json"),
        include_str!("../../../protocol/test-data/action_open_notebook.json"),
        include_str!("../../../protocol/test-data/action_select_cell.json"),
        include_str!("../../../protocol/test-data/action_jupyterlab_cell_insert_text.json"),
        include_str!("../../../protocol/test-data/action_run_cell.json"),
        include_str!("../../../protocol/test-data/action_restart_kernel.json"),
        include_str!("../../../protocol/test-data/action_interrupt_kernel.json"),
        include_str!("../../../protocol/test-data/action_close_file.json"),
        include_str!("../../../protocol/test-data/action_close_preview.json"),
        set_cursor,
    ];

    for fixture in fixtures {
        let envelope = Envelope::parse(fixture).unwrap();
        assert_eq!(envelope.kind, MessageType::Action);
        let _: Action = serde_json::from_value(envelope.payload).unwrap();
    }

    let present = Envelope::parse(fixtures[0]).unwrap();
    let Action::Present = serde_json::from_value(present.payload).unwrap() else {
        panic!("expected Present");
    };

    let delete = Envelope::parse(fixtures[5]).unwrap();
    let Action::DeleteDirectory(action) = serde_json::from_value(delete.payload).unwrap() else {
        panic!("expected DeleteDirectory");
    };
    assert!(!action.recursive);
    assert!(matches!(action.if_missing, MissingTargetPolicy::Ignore));

    let replace = Envelope::parse(fixtures[1]).unwrap();
    let Action::ReplaceText(action) = serde_json::from_value(replace.payload).unwrap() else {
        panic!("expected ReplaceText");
    };
    assert_eq!(
        action.target.start,
        Anchor {
            line: None,
            at: Some(AnchorAt::Text(TextAnchor {
                text: "let x = 5;".to_string(),
                index: None,
            })),
            placement: None,
            eof: false,
        }
    );
    assert_eq!(
        action.target.end,
        Anchor {
            line: None,
            at: Some(AnchorAt::Text(TextAnchor {
                text: "let x = 5;".to_string(),
                index: None,
            })),
            placement: Some(Placement::After),
            eof: false,
        }
    );
    assert_eq!(action.target.expected_text, "let x = 5;");
    assert_eq!(action.text, "let mut x = 5;");
    assert_eq!(
        action.edit_mode,
        Some(ReplaceTextEditMode::OverwriteSelection)
    );
    assert_eq!(action.speed_ms, Some(30));
    assert_eq!(action.target_hold_ms, Some(250));

    let terminal = Envelope::parse(fixtures[7]).unwrap();
    let Action::TerminalCommand(action) = serde_json::from_value(terminal.payload).unwrap() else {
        panic!("expected TerminalCommand");
    };
    assert_eq!(action.accepted_exit_codes, vec![0]);
    assert!(action.completion_marker.is_none());
    assert!(action.new_terminal);

    let controlled_failure = Envelope::parse(fixtures[8]).unwrap();
    let Action::TerminalCommand(action) =
        serde_json::from_value(controlled_failure.payload).unwrap()
    else {
        panic!("expected TerminalCommand");
    };
    assert_eq!(action.accepted_exit_codes, vec![1]);
    assert_eq!(
        action.expected_output,
        vec!["cannot assign twice to immutable variable"]
    );

    let open = Envelope::parse(fixtures[9]).unwrap();
    let Action::OpenFile(action) = serde_json::from_value(open.payload).unwrap() else {
        panic!("expected OpenFile");
    };
    assert_eq!(action.path, "src/main.rs");

    let cursor = Envelope::parse(set_cursor).unwrap();
    let Action::SetCursor(action) = serde_json::from_value(cursor.payload).unwrap() else {
        panic!("expected SetCursor");
    };
    assert_eq!(action.path, "src/main.rs");
    assert_eq!(
        action.target,
        Anchor {
            line: Some(8),
            at: None,
            placement: Some(Placement::After),
            eof: false,
        }
    );

    let insert = Envelope::parse(fixtures[14]).unwrap();
    let Action::InsertText(action) = serde_json::from_value(insert.payload).unwrap() else {
        panic!("expected InsertText");
    };
    assert_eq!(action.path, "src/main.rs");
    assert_eq!(
        action.target,
        cynapse_client_core::InsertTextTarget {
            line: Some(1),
            at: None,
            placement: None,
            eof: false,
        }
    );
    assert_eq!(action.speed_ms, Some(30));
    assert_eq!(action.target_hold_ms, Some(125));
    assert_eq!(action.text, "fn main() {}\n");

    let delete = Envelope::parse(fixtures[15]).unwrap();
    let Action::DeleteText(action) = serde_json::from_value(delete.payload).unwrap() else {
        panic!("expected DeleteText");
    };
    assert_eq!(action.path, "src/main.rs");
    let DeleteTextTarget::Range(range) = action.target else {
        panic!("expected Edit Range");
    };
    assert_eq!(
        range.start,
        Anchor {
            line: None,
            at: Some(AnchorAt::Text(TextAnchor {
                text: "// temporary".to_string(),
                index: Some(0),
            })),
            placement: None,
            eof: false,
        }
    );
    assert_eq!(
        range.end,
        Anchor {
            line: None,
            at: Some(AnchorAt::Text(TextAnchor {
                text: "// temporary".to_string(),
                index: Some(0),
            })),
            placement: Some(Placement::After),
            eof: false,
        }
    );
    assert_eq!(range.expected_text, "// temporary");
    assert_eq!(action.direction, Some(DeleteDirection::Delete));
    assert_eq!(action.target_hold_ms, Some(200));

    let cursor = Envelope::parse(fixtures[16]).unwrap();
    let Action::DeleteText(action) = serde_json::from_value(cursor.payload).unwrap() else {
        panic!("expected DeleteText cursor");
    };
    let DeleteTextTarget::Cursor(target) = action.target else {
        panic!("expected Cursor Deletion");
    };
    assert_eq!(target.cursor.line, 1);
    assert_eq!(target.cursor.column, 0);
    assert_eq!(target.cursor.count, 3);
    assert_eq!(target.expected_text, "fn ");
}

#[test]
fn close_actions_carry_only_the_target_path() {
    let close_file = Envelope::parse(include_str!(
        "../../../protocol/test-data/action_close_file.json"
    ))
    .unwrap();
    let Action::CloseFile(close_file) = serde_json::from_value(close_file.payload).unwrap() else {
        panic!("expected CloseFile");
    };
    assert_eq!(close_file.path, "src/main.rs");

    let close_preview = Envelope::parse(include_str!(
        "../../../protocol/test-data/action_close_preview.json"
    ))
    .unwrap();
    let Action::ClosePreview(close_preview) =
        serde_json::from_value(close_preview.payload).unwrap()
    else {
        panic!("expected ClosePreview");
    };
    assert_eq!(close_preview.path, "README.md");
}

#[test]
fn golden_jupyterlab_actions_preserve_ids_and_defaults() {
    let open = Envelope::parse(include_str!(
        "../../../protocol/test-data/action_open_notebook.json"
    ))
    .unwrap();
    let Action::JupyterLabOpenNotebook(open) = serde_json::from_value(open.payload).unwrap() else {
        panic!("expected JupyterLabOpenNotebook");
    };
    assert_eq!(open.path, "ch/intro/lesson.ipynb");

    let select = Envelope::parse(include_str!(
        "../../../protocol/test-data/action_select_cell.json"
    ))
    .unwrap();
    let Action::JupyterLabCellSelect(select) = serde_json::from_value(select.payload).unwrap()
    else {
        panic!("expected JupyterLabCellSelect");
    };
    assert_eq!(select.cell_id, "cell-print");
    assert!(select.scroll);
    assert_eq!(select.align, Some(JupyterLabCellScrollAlign::TopCenter));

    let run = Envelope::parse(include_str!(
        "../../../protocol/test-data/action_run_cell.json"
    ))
    .unwrap();
    let Action::JupyterLabCellRun(run) = serde_json::from_value(run.payload).unwrap() else {
        panic!("expected JupyterLabCellRun");
    };
    assert_eq!(run.from_cell_id.as_deref(), Some("cell-print"));
    assert_eq!(run.to_cell_id.as_deref(), Some("cell-print"));
    assert_eq!(run.timeout_ms, 120_000);

    let restart = Envelope::parse(include_str!(
        "../../../protocol/test-data/action_restart_kernel.json"
    ))
    .unwrap();
    let Action::JupyterLabRestartKernel(restart) = serde_json::from_value(restart.payload).unwrap()
    else {
        panic!("expected JupyterLabRestartKernel");
    };
    assert_eq!(restart.path, "ch/intro/lesson.ipynb");

    let interrupt = Envelope::parse(include_str!(
        "../../../protocol/test-data/action_interrupt_kernel.json"
    ))
    .unwrap();
    let Action::JupyterLabInterruptKernel(interrupt) =
        serde_json::from_value(interrupt.payload).unwrap()
    else {
        panic!("expected JupyterLabInterruptKernel");
    };
    assert_eq!(interrupt.path, "ch/intro/lesson.ipynb");
}

#[test]
fn golden_run_cell_ack_carries_only_execution_summaries() {
    let envelope = Envelope::parse(include_str!(
        "../../../protocol/test-data/ack_run_cell.json"
    ))
    .unwrap();

    assert_eq!(envelope.kind, MessageType::Ack);
    assert_eq!(envelope.payload["status"], "done");
    assert_eq!(envelope.payload["execution_count"], 7);
    assert_eq!(envelope.payload["error"]["ename"], "ValueError");
    assert_eq!(
        envelope.payload["error"]["traceback"][1],
        "ValueError: bad input"
    );
    assert_eq!(
        envelope.payload["outputs"][0]["mime_types"],
        serde_json::json!([])
    );
    assert_eq!(
        envelope.payload["outputs"][1]["mime_types"],
        serde_json::json!(["image/png", "text/plain"])
    );
    assert_eq!(envelope.payload["outputs"][1]["size"], 4812);

    // Protocol v2 is additive: the Client's AckPayload ignores the new
    // execution-summary fields it does not read yet.
    let client_payload = serde_json::from_value::<AckPayload>(envelope.payload).unwrap();
    assert_eq!(client_payload.status, AckStatus::Done);
    assert_eq!(
        client_payload.output.as_deref(),
        Some("ValueError: bad input\n")
    );
    assert_eq!(client_payload.output_truncated, Some(false));
}

#[test]
fn every_shared_golden_message_matches_the_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../protocol/schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut testdata_files = Vec::new();
    collect_json_testdata(
        &project_root.join("protocol/test-data"),
        &mut testdata_files,
    );
    testdata_files.sort();
    assert!(testdata_files.len() >= 9);

    for testdata_file in &testdata_files {
        let instance: serde_json::Value =
            serde_json::from_slice(&std::fs::read(testdata_file).unwrap()).unwrap();
        assert!(
            validator.is_valid(&instance),
            "{} does not match protocol/schema.json: {}",
            testdata_file.display(),
            validator
                .iter_errors(&instance)
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}

#[test]
fn representative_invalid_protocol_fixtures_fail_the_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../protocol/schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut testdata_files = Vec::new();
    collect_json_testdata(
        &project_root.join("protocol/invalid-test-data"),
        &mut testdata_files,
    );
    testdata_files.sort();
    assert!(testdata_files.len() >= 5);

    for testdata_file in &testdata_files {
        let instance: serde_json::Value =
            serde_json::from_slice(&std::fs::read(testdata_file).unwrap()).unwrap();
        assert!(
            !validator.is_valid(&instance),
            "{} unexpectedly matches protocol/schema.json",
            testdata_file.display()
        );
    }
}

#[test]
fn runtime_discovery_fixture_matches_the_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../protocol/schema.json")).unwrap();
    let validator = jsonschema::validator_for(&serde_json::json!({
        "$schema": schema["$schema"].clone(),
        "$ref": "#/$defs/runtime_discovery",
        "$defs": schema["$defs"].clone(),
    }))
    .unwrap();
    let instance: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/contracts/runtime-discovery.json"
    ))
    .unwrap();

    assert!(
        validator.is_valid(&instance),
        "runtime discovery fixture does not match protocol/schema.json"
    );
    let discovery: cynapse_client_core::DiscoveryFile = serde_json::from_value(instance).unwrap();
    assert_eq!(discovery.port, 49152);
    assert_eq!(discovery.token.len(), 36);
}

fn collect_json_testdata(
    directory: &std::path::Path,
    testdata_files: &mut Vec<std::path::PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json_testdata(&path, testdata_files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            testdata_files.push(path);
        }
    }
}
