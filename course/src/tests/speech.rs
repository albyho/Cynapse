//! Speakable narration text from Caption markup.

use super::*;
use serde_json::json;

#[test]
fn speech_aliases_match_the_python_assembler() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../test-data/caption_markup_cases.json")).unwrap();
    for case in cases {
        let clip = json!({"caption": case["caption"]});
        let result = narration_clip_speech_text(&clip);
        if let Some(expected) = case["spoken"].as_str() {
            assert_eq!(result.unwrap(), expected, "{clip}");
        } else if let Some(expected) = case["speech_error"].as_str().or(case["error"].as_str()) {
            let error = result.unwrap_err();
            assert!(error.contains(expected), "{clip}: {error}");
        }
    }
}

#[test]
fn legacy_speech_edits_are_rejected_even_when_empty_or_null() {
    for edits in [
        json!(null),
        json!([]),
        json!([{"i": 0, "s": "SQL", "p": "sequel"}]),
    ] {
        let error = narration_clip_speech_text(&json!({"caption": "SQL", "speech_edits": edits}))
            .unwrap_err();
        assert!(error.contains("speech_edits"), "{error}");
    }
}

#[test]
fn a_clip_without_a_caption_is_rejected() {
    let error = narration_clip_speech_text(&json!({})).unwrap_err();
    assert!(error.contains("caption"), "{error}");
}
