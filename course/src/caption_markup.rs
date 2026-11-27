//! Caption text, emphasis, speech aliases, and inline formulas.

/// A run of Caption text or the complete source of a Caption Formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionSegment {
    pub text: String,
    pub emphasized: bool,
    pub formula: bool,
}

#[derive(Default)]
struct Scan {
    segments: Vec<CaptionSegment>,
    spoken: String,
    uncovered_formula: Option<String>,
}

/// Splits a Caption into display runs. Malformed markup remains visible.
pub fn caption_segments(caption: &str) -> Vec<CaptionSegment> {
    scan(caption).map_or_else(
        |_| {
            vec![CaptionSegment {
                text: caption.to_owned(),
                emphasized: false,
                formula: false,
            }]
        },
        |scan| {
            let mut display: Vec<CaptionSegment> = Vec::new();
            for segment in scan.segments {
                if let Some(previous) = display.last_mut()
                    && !previous.formula
                    && !segment.formula
                    && previous.emphasized == segment.emphasized
                {
                    previous.text.push_str(&segment.text);
                } else {
                    display.push(segment);
                }
            }
            display
        },
    )
}

/// Validate markup and supported formulas independently of synthesis.
pub fn validate_caption_markup(caption: &str) -> Result<(), String> {
    for segment in scan(caption)?
        .segments
        .iter()
        .filter(|segment| segment.formula)
    {
        caption_formula_layout(&segment.text)?;
    }
    Ok(())
}
/// Layout is shared with package validation so supported formulas are the same.
pub fn caption_formula_layout(source: &str) -> Result<ratex_layout::LayoutBox, String> {
    let math = source
        .strip_prefix(r"\(")
        .and_then(|source| source.strip_suffix(r"\)"))
        .filter(|math| !math.trim().is_empty())
        .ok_or_else(|| format!("invalid or empty Caption formula `{source}`"))?;
    let error = |reason: &dyn std::fmt::Display| format!("Caption formula `{source}`: {reason}");
    if math.len() > 4096 {
        return Err(error(&"source exceeds 4096 bytes"));
    }
    if math.split('\\').skip(1).any(|part| {
        let end = part
            .find(|character: char| !character.is_ascii_alphabetic())
            .unwrap_or(part.len());
        matches!(
            &part[..end],
            "def"
                | "gdef"
                | "edef"
                | "xdef"
                | "let"
                | "futurelet"
                | "global"
                | "long"
                | "newcommand"
                | "renewcommand"
                | "providecommand"
        )
    }) {
        return Err(error(&"custom macros are unsupported"));
    }
    let nodes = ratex_parser::parser::parse(math).map_err(|reason| error(&reason))?;
    let options =
        ratex_layout::LayoutOptions::default().with_style(ratex_types::math_style::MathStyle::Text);
    let layout = ratex_layout::layout(&nodes, &options);
    ratex_layout::validate_layout(&layout).map_err(|reason| error(&reason))?;
    if !(0.0..=64.0).contains(&layout.width)
        || !(0.0..=12.0).contains(&layout.height)
        || !(0.0..=12.0).contains(&layout.depth)
        || layout.height + layout.depth > 12.0
    {
        return Err(error(&"layout exceeds Caption formula dimensions"));
    }
    Ok(layout)
}

/// Returns the visible wording with whitespace normalized outside formulas.
pub fn visible_caption_text(caption: &str) -> String {
    let segments = caption_segments(caption);
    let mut text = String::new();
    let mut space = false;
    for segment in segments {
        if segment.formula {
            if space {
                text.push(' ');
            }
            text.push_str(&segment.text);
            space = false;
        } else {
            for character in segment.text.chars() {
                if character.is_whitespace() {
                    space = !text.is_empty();
                } else {
                    if space {
                        text.push(' ');
                    }
                    text.push(character);
                    space = false;
                }
            }
        }
    }
    text
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Emphasis,
    Sub,
}

enum Tag {
    Open(TagKind, Option<String>),
    Close(TagKind),
}

struct Span {
    kind: TagKind,
    alias: Option<String>,
    first_segment: usize,
}

fn tag_head(source: &str) -> Option<(bool, &str, &str)> {
    let rest = source.strip_prefix('<')?;
    let closing = rest.starts_with('/');
    let name_and_tail = rest.strip_prefix('/').unwrap_or(rest);
    let name_end = name_and_tail
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':')))
        .unwrap_or(name_and_tail.len());
    let (name, tail) = name_and_tail.split_at(name_end);
    let lower = name.to_ascii_lowercase();
    (matches!(lower.as_str(), "emphasis" | "sub")
        || (lower == "c" && tail.starts_with('>'))
        || lower.starts_with("c."))
    .then_some((closing, name, tail))
}

fn parse_tag(source: &str) -> Option<Result<(Tag, usize), String>> {
    const TAG_WHITESPACE: [char; 4] = [' ', '\t', '\r', '\n'];
    let (closing, name, tail) = tag_head(source)?;
    Some((|| {
        let (kind, expected, attribute) = match name.to_ascii_lowercase().as_str() {
            "emphasis" => (TagKind::Emphasis, "emphasis", "level"),
            "sub" => (TagKind::Sub, "sub", "alias"),
            _ => return Err(format!("legacy Caption tag `{name}` is unsupported")),
        };
        if name != expected {
            return Err(format!(
                "Caption tag `{name}` is case-sensitive; use `{expected}`"
            ));
        }
        let remaining = tail.trim_start_matches(TAG_WHITESPACE);
        if closing {
            let after = remaining
                .strip_prefix('>')
                .ok_or_else(|| format!("invalid closing Caption tag `{name}`"))?;
            return Ok((Tag::Close(kind), source.len() - after.len()));
        }
        if remaining.len() == tail.len() {
            return Err(format!(
                "Caption tag `{name}` requires the `{attribute}` attribute"
            ));
        }
        let value = remaining
            .strip_prefix(attribute)
            .and_then(|rest| rest.trim_start_matches(TAG_WHITESPACE).strip_prefix('='))
            .ok_or_else(|| format!("Caption tag `{name}` requires the `{attribute}` attribute"))?
            .trim_start_matches(TAG_WHITESPACE)
            .strip_prefix('"')
            .ok_or_else(|| format!("Caption attribute `{attribute}` requires double quotes"))?;
        let (value, remainder) = value
            .split_once('"')
            .ok_or_else(|| format!("unclosed Caption attribute `{attribute}`"))?;
        let after = remainder
            .trim_start_matches(TAG_WHITESPACE)
            .strip_prefix('>')
            .ok_or_else(|| format!("unclosed Caption tag `{name}` or unexpected attribute"))?;
        let alias = match kind {
            TagKind::Emphasis if value == "strong" => None,
            TagKind::Emphasis => return Err("Caption emphasis level must be `strong`".into()),
            TagKind::Sub => {
                if value.contains('<') {
                    return Err("invalid Caption alias; escape `<` as `&lt;`".into());
                }
                let value = decode_entities(value);
                if value.trim().is_empty() {
                    return Err("empty Caption alias".into());
                }
                Some(value)
            }
        };
        Ok((Tag::Open(kind, alias), source.len() - after.len()))
    })())
}

fn decode_entities(text: &str) -> String {
    // Decode ampersands last so text produced by an escape is never reinterpreted.
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn push_text(scan: &mut Scan, current: &mut String, emphasized: bool, aliased: bool) {
    if current.is_empty() {
        return;
    }
    let text = decode_entities(current);
    current.clear();
    if !aliased {
        scan.spoken.push_str(&text);
    }
    scan.segments.push(CaptionSegment {
        text,
        emphasized,
        formula: false,
    });
}

/// Walk the source once for both display and synthesis. Span boundaries affect
/// the displayed runs and the spoken text independently.
fn scan(caption: &str) -> Result<Scan, String> {
    let mut scan = Scan::default();
    let mut current = String::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut emphasized = false;
    let mut aliased = false;
    let mut offset = 0;

    while let Some(relative_start) = caption[offset..].find(['<', '\\']) {
        let start = offset + relative_start;
        current.push_str(&caption[offset..start]);
        let rest = &caption[start..];

        if rest.starts_with('<') {
            let Some(tag) = parse_tag(rest) else {
                current.push('<');
                offset = start + 1;
                continue;
            };
            let (tag, length) = tag.map_err(|error| {
                format!(
                    "{error} at character {}",
                    caption[..start].chars().count() + 1
                )
            })?;
            push_text(&mut scan, &mut current, emphasized, aliased);
            match tag {
                Tag::Open(kind, alias) => {
                    if spans.iter().any(|span| span.kind == kind) {
                        return Err("nested Caption tags of the same type are unsupported".into());
                    }
                    spans.push(Span {
                        kind,
                        alias,
                        first_segment: scan.segments.len(),
                    });
                }
                Tag::Close(kind) => {
                    let span = spans
                        .pop()
                        .filter(|span| span.kind == kind)
                        .ok_or("unexpected or crossing closing Caption tag")?;
                    if scan.segments[span.first_segment..]
                        .iter()
                        .all(|segment| segment.text.trim().is_empty())
                    {
                        return Err("empty Caption span".into());
                    }
                    if let Some(alias) = span.alias {
                        scan.spoken.push_str(&alias);
                    }
                }
            }
            emphasized = spans.iter().any(|span| span.kind == TagKind::Emphasis);
            aliased = spans.iter().any(|span| span.kind == TagKind::Sub);
            offset = start + length;
            continue;
        }

        let slashes = rest.bytes().take_while(|byte| *byte == b'\\').count();
        let next = rest.as_bytes().get(slashes).copied();
        if !matches!(next, Some(b'(' | b')')) {
            current.push_str(&rest[..slashes]);
            offset = start + slashes;
        } else if slashes % 2 == 0 {
            current.push_str(&rest[1..=slashes]);
            offset = start + slashes + 1;
        } else if slashes > 1 {
            current.push_str(&rest[..slashes - 1]);
            offset = start + slashes - 1;
        } else if next == Some(b')') {
            return Err(format!(
                "unmatched Caption formula delimiter at character {}",
                caption[..start].chars().count() + 1
            ));
        } else {
            let length =
                formula_length(rest).ok_or_else(|| format!("unclosed Caption formula `{rest}`"))?;
            let source = &rest[..length];
            if source[2..length - 2].trim().is_empty() {
                return Err(format!("empty Caption formula `{source}`"));
            }
            if source
                .match_indices('<')
                .any(|(index, _)| tag_head(&source[index..]).is_some())
            {
                return Err(format!("markup inside Caption formula `{source}`"));
            }
            push_text(&mut scan, &mut current, emphasized, aliased);
            if !aliased && scan.uncovered_formula.is_none() {
                scan.uncovered_formula = Some(source.to_owned());
            }
            scan.segments.push(CaptionSegment {
                text: source.to_owned(),
                emphasized,
                formula: true,
            });
            offset = start + length;
        }
    }
    if !spans.is_empty() {
        return Err("unclosed Caption tag".into());
    }
    current.push_str(&caption[offset..]);
    push_text(&mut scan, &mut current, emphasized, aliased);
    Ok(scan)
}

fn formula_length(source: &str) -> Option<usize> {
    let mut offset = 2;
    while let Some(relative) = source[offset..].find('\\') {
        offset += relative;
        let slashes = source[offset..]
            .bytes()
            .take_while(|byte| *byte == b'\\')
            .count();
        offset += slashes;
        if slashes % 2 == 1 && source.as_bytes().get(offset) == Some(&b')') {
            return Some(offset + 1);
        }
    }
    None
}

/// Prepare plain-text speech for a Clip selected for synthesis.
pub fn narration_clip_speech_text(clip: &serde_json::Value) -> Result<String, String> {
    if clip.get("speech_edits").is_some() {
        return Err("legacy `speech_edits` is unsupported; use an inline Speech Alias".into());
    }
    let caption = clip
        .get("caption")
        .and_then(serde_json::Value::as_str)
        .ok_or("narration Clip must contain a string `caption`")?;
    let scan = scan(caption)?;
    if let Some(formula) = scan.uncovered_formula {
        return Err(format!(
            "Caption formula `{formula}` requires a Speech Alias"
        ));
    }
    let spoken = scan.spoken.split_whitespace().collect::<Vec<_>>().join(" ");
    if spoken.is_empty() {
        return Err("narration Clip has no speakable caption text".into());
    }
    Ok(spoken)
}

#[cfg(test)]
mod tests;
