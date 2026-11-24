use super::{CaptionSegment, caption_segments, validate_caption_markup, visible_caption_text};

#[test]
fn validates_inline_caption_formulas() {
    assert!(validate_caption_markup(r"金额 $5，<c.aqua>\(\frac{a}{b}\)</c>").is_ok());
    assert!(validate_caption_markup(r"残缺 \(x").is_err());
    assert!(validate_caption_markup(r"空 \( \)").is_err());
    assert!(validate_caption_markup(r"\(\unsupportedcommand{x}\)").is_err());
    assert!(validate_caption_markup(r"\(\rule{1em}{1000000em}\)").is_err());
    assert!(validate_caption_markup(r"\(x<c.aqua>+y</c>\)").is_err());
    assert_eq!(visible_caption_text(r"字面 \\("), r"字面 \(");
}

fn segment(text: &str, aqua: bool) -> CaptionSegment {
    CaptionSegment {
        formula: false,
        text: text.to_string(),
        aqua,
    }
}

#[test]
fn segments_split_plain_and_emphasized_runs() {
    assert_eq!(
        caption_segments("<c.aqua>one</c.aqua> two <C.AQUA>three</c>"),
        vec![
            segment("one", true),
            segment(" two ", false),
            segment("three", true)
        ]
    );
}

#[test]
fn segments_keep_plain_angle_brackets_literal() {
    assert_eq!(
        caption_segments("a < b and Vec<T>"),
        vec![segment("a < b and Vec<T>", false)]
    );
    assert!(caption_segments("").is_empty());
}

#[test]
fn segments_keep_rejected_tags_visible() {
    // Invalid markup still renders its own characters: a renderer must not
    // swallow text, and packaging rejects these Captions before shipping.
    assert_eq!(
        caption_segments("<c.red>x</c.red>"),
        vec![segment("<c.red>x</c.red>", false)]
    );
}

#[test]
fn segments_treat_stray_lt_before_markup_as_text() {
    // Rendering and validation share one scan, so `a < b` cannot swallow the
    // emphasis span that follows it.
    assert_eq!(
        caption_segments("先判断 a < b，再看 <c.aqua>重点</c.aqua>。"),
        vec![
            segment("先判断 a < b，再看 ", false),
            segment("重点", true),
            segment("。", false),
        ]
    );
}

#[derive(serde::Deserialize)]
struct SharedCase {
    caption: String,
    valid: bool,
    #[serde(default)]
    visible: Option<String>,
}

fn assert_valid(caption: &str) {
    assert_eq!(
        validate_caption_markup(caption),
        Ok(()),
        "expected a valid caption: {caption}"
    );
}

fn assert_invalid(caption: &str, expected: &str) {
    let error = validate_caption_markup(caption).expect_err("expected invalid caption markup");
    assert!(
        error.contains(expected),
        "expected error containing {expected:?}, got {error:?}"
    );
}

#[test]
fn plain_text_is_valid() {
    assert_valid("重点：变量遮蔽");
    assert_valid("");
}

#[test]
fn both_close_forms_are_valid() {
    assert_valid("重点：<c.aqua>变量遮蔽</c.aqua>。");
    assert_valid("重点：<c.aqua>变量遮蔽</c>。");
}

#[test]
fn multiple_and_adjacent_spans_are_valid() {
    assert_valid("<c.aqua>可变性</c>和<c.aqua>作用域</c>");
    assert_valid("<c.aqua>a</c><c.aqua>b</c>");
    assert_valid("<c.aqua>整句都是重点</c>");
}

#[test]
fn tags_are_case_insensitive() {
    assert_valid("<C.AQUA>重点</C.AQUA>");
    assert_valid("<C.AQUA>重点</c>");
}

#[test]
fn non_caption_angle_brackets_are_plain_text() {
    assert_valid("用 Vec<T> 存值，判断 a < b。");
    assert_valid("<code> 不是字幕标签");
    assert_valid("先判断 a < b，再看 <c.aqua>重点</c.aqua>。");
    assert_valid("先判断 a < b > c，再看 <c.aqua>重点</c.aqua>。");
}

#[test]
fn unclosed_tag_is_rejected() {
    assert_invalid("<c.aqua>未闭合", "unclosed");
}

#[test]
fn caption_tag_without_gt_is_rejected() {
    assert_invalid("重点：<c.aqua", "unclosed");
    assert_invalid("重点：</c.aqua", "unclosed");
    assert_invalid("重点：<C.AQUA", "unclosed");
}

#[test]
fn stray_close_tag_is_rejected() {
    assert_invalid("先看<c.aqua>重点</c>，再补一个</c>。", "no matching");
}

#[test]
fn nested_tags_are_rejected() {
    assert_invalid(
        "<c.aqua>外<c.aqua>内</c>外</c>",
        "emphasis spans cannot nest",
    );
}

#[test]
fn empty_span_is_rejected() {
    assert_invalid("重点：<c.aqua></c>", "empty emphasis span");
    assert_invalid("重点：<c.aqua> \t</c.aqua>", "empty emphasis span");
}

#[test]
fn unknown_caption_tags_are_rejected() {
    assert_invalid("重点：<c.red>遮蔽</c>", "unknown caption tag");
    assert_invalid("重点：<c.aqua>遮蔽</c.aqu>", "unknown caption tag");
    assert_invalid("重点：<c>遮蔽</c>", "unknown caption tag");
    assert_invalid("重点：<c.aqua>遮蔽</c.aqua >", "unknown caption tag");
}

#[test]
fn visible_caption_text_removes_emphasis_and_normalizes_whitespace() {
    assert_eq!(
        visible_caption_text("  输出\n<c.aqua>{}。</c.aqua>  "),
        "输出 {}。"
    );
    assert_eq!(visible_caption_text("a < b"), "a < b");
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
            assert!(
                validation.is_ok(),
                "expected a valid caption: {caption} ({validation:?})"
            );
            let expected = case
                .visible
                .as_deref()
                .expect("valid shared case needs visible text");
            assert_eq!(
                visible_caption_text(caption),
                expected,
                "visible text for {caption}"
            );
        } else {
            assert!(
                validation.is_err(),
                "expected an invalid caption: {caption}"
            );
        }
    }
}
