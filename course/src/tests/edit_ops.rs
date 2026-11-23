//! Pure code-edit semantics shared by the Client scenario harness and tests.

use super::*;
use serde_json::{Value, json};

fn insert(target: Value, code: &str) -> InsertTextAction {
    serde_json::from_value(json!({
        "path": "main.rs",
        "target": target,
        "text": code
    }))
    .unwrap()
}

fn delete(target: Value) -> DeleteTextAction {
    serde_json::from_value(json!({
        "path": "main.rs",
        "target": target,
        "direction": "backspace"
    }))
    .unwrap()
}

fn replace(target: Value, code: &str) -> ReplaceTextAction {
    serde_json::from_value(json!({
        "path": "main.rs",
        "target": target,
        "text": code
    }))
    .unwrap()
}

#[test]
fn suffix_prefix_len_returns_the_longest_suffix_prefix() {
    assert_eq!(suffix_prefix_len("abc", "abcd"), 3);
    assert_eq!(suffix_prefix_len("abcx", "abcd"), 0);
    assert_eq!(suffix_prefix_len("xabc", "abc"), 3);
    assert_eq!(suffix_prefix_len("校验", "校验x"), "校验".len());
    assert_eq!(suffix_prefix_len("", "abcd"), 0);
    assert_eq!(suffix_prefix_len("abc", ""), 0);
}

#[test]
fn insert_text_at_a_position() {
    let action = insert(json!({"line": 2, "at": 0}), "    println!(\"hi\");\n");

    let updated = apply_insert_text("fn main() {\n}\n", &action).unwrap();

    assert_eq!(updated, "fn main() {\n    println!(\"hi\");\n}\n");
}

#[test]
fn an_omitted_literal_index_uses_the_first_occurrence() {
    let action = insert(json!({"at": {"text": "x"}}), "!");

    let updated = apply_insert_text("let x = x;\n", &action).unwrap();

    assert_eq!(updated, "let !x = x;\n");
}

#[test]
fn inserting_at_end_of_file_is_idempotent() {
    let action = insert(json!({"eof": true}), "b");

    let once = apply_insert_text("a", &action).unwrap();
    let twice = apply_insert_text(&once, &action).unwrap();

    assert_eq!(once, "ab");
    assert_eq!(twice, "ab");
}

#[test]
fn delete_text_removes_a_text_range() {
    let action = delete(json!({
        "start": {"at": {"text": "    body"}},
        "end": {"at": {"text": "    body"}, "placement": "after"},
        "expected_text": "    body\n"
    }));

    let updated = apply_delete_text("fn main() {\n    body\n}\n", &action).unwrap();

    assert_eq!(updated, "fn main() {\n}\n");
}

#[test]
fn delete_text_can_use_a_cursor() {
    let action = delete(json!({
        "cursor": {"line": 1, "column": 3, "count": 1},
        "expected_text": "c"
    }));

    let updated = apply_delete_text("abc", &action).unwrap();

    assert_eq!(updated, "ab");
}

#[test]
fn replace_text_replaces_the_expected_range() {
    let action = replace(
        json!({
            "start": {"at": {"text": "1"}},
            "end": {"at": {"text": "1"}, "placement": "after"},
            "expected_text": "1"
        }),
        "2",
    );

    let updated = apply_replace_text("let x = 1;\n", &action).unwrap();

    assert_eq!(updated, "let x = 2;\n");
}

#[test]
fn replace_text_uses_the_first_text_occurrence_by_default() {
    let action = replace(
        json!({
            "start": {"at": {"text": "x"}},
            "end": {"at": {"text": "x"}},
            "expected_text": "x"
        }),
        "y",
    );

    let updated = apply_replace_text("x = x;\n", &action).unwrap();

    assert_eq!(updated, "y = x;\n");
}

#[test]
fn replace_replay_rejects_a_coordinate_anchor_out_of_range() {
    let action = replace(
        json!({
            "start": {"line": 9},
            "end": {"line": 9},
            "expected_text": "let gone = 0;"
        }),
        "let mut x = 5;",
    );

    let error = apply_replace_text("let mut x = 5;\n", &action).unwrap_err();

    assert!(error.0.contains("outside the document"), "{error}");
}

#[test]
fn replace_replay_rejects_when_expected_text_remains() {
    let action = replace(
        json!({
            "start": {"at": {"text": "let y = 5;"}},
            "end": {"at": {"text": "let y = 5;"}, "placement": "after"},
            "expected_text": "x = 5;"
        }),
        "let mut x = 5;",
    );

    let error = apply_replace_text("let mut x = 5;\n", &action).unwrap_err();

    assert!(error.0.contains("not found"), "{error}");
}

#[test]
fn code_edits_normalize_line_endings() {
    let action = insert(json!({"line": 2, "at": 0}), "c\n");

    let updated = apply_insert_text("a\r\nb\r\n", &action).unwrap();

    assert_eq!(updated, "a\nc\nb\n");
}

#[test]
fn a_mismatched_edit_range_is_rejected() {
    let action = delete(json!({
        "start": {"at": {"text": "body"}},
        "end": {"at": {"text": "body"}, "placement": "after"},
        "expected_text": "something else"
    }));

    let error = apply_delete_text("fn main() {\n    body\n}\n", &action).unwrap_err();

    assert!(error.0.contains("expected_text"), "{error}");
}

#[test]
fn line_end_insert_stays_before_the_line_break() {
    let at_line_end = insert(json!({"line": 1, "placement": "after"}), "!");
    assert_eq!(apply_insert_text("a\nb", &at_line_end).unwrap(), "a!\nb");

    let next_line = insert(json!({"line": 2}), "!");
    assert_eq!(apply_insert_text("a\nb", &next_line).unwrap(), "a\n!b");

    let eof = insert(json!({"eof": true}), "!");
    assert_eq!(apply_insert_text("a\nb", &eof).unwrap(), "a\nb!");
}

#[test]
fn text_anchors_are_line_scoped_and_indexed() {
    let action = insert(
        json!({"line": 2, "at": {"text": "x", "index": 1}, "placement": "after"}),
        "!",
    );
    assert_eq!(
        apply_insert_text("x = 1;\nx = x;\n", &action).unwrap(),
        "x = 1;\nx = x!;\n"
    );

    let first = insert(json!({"at": {"text": "x"}}), "!");
    assert_eq!(apply_insert_text("x = x;\n", &first).unwrap(), "!x = x;\n");
}

#[test]
fn range_end_at_a_line_end_includes_the_break() {
    let action = replace(
        json!({
            "start": {"line": 1},
            "end": {"line": 1, "placement": "after"},
            "expected_text": "a\n"
        }),
        "A\n",
    );
    assert_eq!(apply_replace_text("a\nb\n", &action).unwrap(), "A\nb\n");

    let without_break = replace(
        json!({
            "start": {"line": 1},
            "end": {"line": 1, "placement": "after"},
            "expected_text": "a"
        }),
        "A",
    );
    assert!(apply_replace_text("a\nb\n", &without_break).is_err());
}

#[test]
fn column_end_after_on_an_empty_line_includes_the_break() {
    let action = delete(json!({
        "start": {"line": 2, "at": 0, "placement": "after"},
        "end": {"line": 2, "at": 0, "placement": "after"},
        "expected_text": "\n"
    }));

    assert_eq!(apply_delete_text("a\n\nb", &action).unwrap(), "a\nb");
}

#[test]
fn range_end_at_a_line_start_does_not_add_a_break() {
    let action = delete(json!({
        "start": {"line": 1, "at": 1},
        "end": {"line": 2, "placement": "before"},
        "expected_text": "\n"
    }));
    assert_eq!(apply_delete_text("a\nb\n", &action).unwrap(), "ab\n");
}

#[test]
fn text_anchor_placements_select_the_expected_boundaries() {
    let action = delete(json!({
        "start": {"at": {"text": "abc", "index": 0}, "placement": "after"},
        "end": {"at": {"text": "abc", "index": 1}, "placement": "before"},
        "expected_text": " "
    }));
    assert_eq!(apply_delete_text("abc abc\n", &action).unwrap(), "abcabc\n");
}

#[test]
fn shared_text_edit_cases_match_the_adapter() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/contracts/text-edit-cases.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let text = case["text"].as_str().unwrap();
        let action: Action = serde_json::from_value(case["action"].clone()).unwrap();
        let actual = match &action {
            Action::InsertText(action) => apply_insert_text(text, action),
            Action::DeleteText(action) => apply_delete_text(text, action),
            Action::ReplaceText(action) => apply_replace_text(text, action),
            _ => panic!("{name}: unexpected action"),
        };
        assert_eq!(
            actual.unwrap(),
            case["expected"]["text"].as_str().unwrap(),
            "{name}"
        );
    }
}
