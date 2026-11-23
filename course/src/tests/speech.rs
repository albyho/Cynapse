//! Speakable narration text: emphasis markup and `speech_edits`.

use super::*;
use serde_json::json;

#[test]
fn emphasis_markup_is_removed_and_whitespace_is_collapsed() {
    let clip = json!({
        "caption": "  欢迎  来到 <c.aqua>Cynapse Studio</c>。 "
    });

    let spoken = narration_clip_speech_text(&clip).unwrap();

    assert_eq!(spoken, "欢迎 来到 Cynapse Studio。");
}

#[test]
fn speech_edits_replace_exact_character_ranges() {
    let clip = json!({
        "caption": "重庆银行行长",
        "speech_edits": [
            { "i": 0, "s": "重", "p": "CHONG2" },
            { "i": 4, "s": "行", "p": "HANG2" },
            { "i": 5, "s": "长", "p": "ZHANG3" }
        ]
    });

    let spoken = narration_clip_speech_text(&clip).unwrap();

    assert_eq!(spoken, "CHONG2庆银行HANG2ZHANG3");
}

#[test]
fn speech_edits_must_match_the_visible_text() {
    let clip = json!({
        "caption": "hello",
        "speech_edits": [{ "i": 1, "s": "x", "p": "y" }]
    });

    let error = narration_clip_speech_text(&clip).unwrap_err();

    assert!(error.contains("does not match"), "{error}");
}

#[test]
fn overlapping_speech_edits_are_rejected() {
    let clip = json!({
        "caption": "abcdef",
        "speech_edits": [
            { "i": 0, "s": "abc", "p": "x" },
            { "i": 2, "s": "cde", "p": "y" }
        ]
    });

    let error = narration_clip_speech_text(&clip).unwrap_err();

    assert!(error.contains("overlap"), "{error}");
}

#[test]
fn a_clip_without_a_caption_is_rejected() {
    let error = narration_clip_speech_text(&json!({})).unwrap_err();

    assert!(error.contains("caption"), "{error}");
}
