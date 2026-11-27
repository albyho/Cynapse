use super::{CaptionSegment, caption_segments, validate_caption_markup, visible_caption_text};

#[test]
fn validates_inline_caption_formulas() {
    assert!(
        validate_caption_markup(r#"金额 $5，<emphasis level="strong">\(\frac{a}{b}\)</emphasis>"#)
            .is_ok()
    );
    assert!(validate_caption_markup(r"空 \( \)").is_err());
    assert!(validate_caption_markup(r"\(\unsupportedcommand{x}\)").is_err());
    assert!(validate_caption_markup(r"\(\rule{1em}{1000000em}\)").is_err());
}

fn segment(text: &str, emphasized: bool) -> CaptionSegment {
    CaptionSegment {
        formula: false,
        text: text.to_string(),
        emphasized,
    }
}

#[test]
fn renders_only_original_words_and_emphasizes_the_marked_subset() {
    assert_eq!(
        caption_segments(
            r#"先判断 a < b，再看 <sub alias="sequel"><emphasis level="strong">SQL</emphasis> 查询</sub>。"#
        ),
        vec![
            segment("先判断 a < b，再看 ", false),
            segment("SQL", true),
            segment(" 查询。", false)
        ]
    );
    assert_eq!(
        caption_segments(
            r#"<emphasis level="strong"><sub alias="sequel">SQL</sub> 查询</emphasis>"#
        ),
        vec![segment("SQL 查询", true)]
    );
    assert!(caption_segments("").is_empty());
}

#[test]
fn keeps_malformed_markup_visible() {
    for caption in ["<c.aqua>旧</c.aqua>", r#"<sub alias="say">text"#] {
        assert_eq!(caption_segments(caption), vec![segment(caption, false)]);
    }
}

#[derive(serde::Deserialize)]
struct SharedCase {
    caption: String,
    valid: bool,
    visible: Option<String>,
    error: Option<String>,
}

#[test]
fn shared_python_cases_match() {
    let cases: Vec<SharedCase> =
        serde_json::from_str(include_str!("../../test-data/caption_markup_cases.json"))
            .expect("shared caption markup fixture");
    for case in cases {
        let caption = case.caption.as_str();
        let validation = validate_caption_markup(caption);
        if case.valid {
            assert_eq!(validation, Ok(()), "{caption}");
            assert_eq!(
                visible_caption_text(caption),
                case.visible.unwrap(),
                "{caption}"
            );
        } else {
            let error = validation.expect_err(caption);
            assert!(error.contains(&case.error.unwrap()), "{caption}: {error}");
        }
    }
}
