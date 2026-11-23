//! Caption emphasis markup: one scanner, three outputs.
//!
//! A Caption is plain text plus a single emphasis form: `<c.aqua>…</c.aqua>` or
//! `<c.aqua>…</c>`. [`scan`] is the only implementation of that grammar in the
//! repository: packaging validation, the visible text that speech edits index
//! into, and the Client's rendering runs all come from it. Anything else in
//! angle brackets, such as `Vec<T>` or `a < b`, is plain Caption text.

const OPEN_TAG: &str = "<c.aqua>";

/// A run of Caption text, either plain or emphasized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionSegment {
    pub text: String,
    pub aqua: bool,
}

struct Scan {
    segments: Vec<CaptionSegment>,
    error: Option<String>,
}

/// Splits a Caption into plain and emphasized runs for rendering.
///
/// Malformed markup keeps its characters as literal text: a renderer must never
/// drop learner-visible text, and packaging rejects these Captions before they
/// reach a Client anyway.
pub fn caption_segments(caption: &str) -> Vec<CaptionSegment> {
    scan(caption).segments
}

/// Checks the Caption emphasis markup that packaging must reject.
///
/// Valid: well-formed, non-empty `<c.aqua>` spans with either close form.
/// Invalid: unclosed or stray tags, nesting, empty spans, and unknown tags in
/// the `<c.…>` namespace. Any other angle-bracket text, such as `Vec<T>` or
/// `a < b`, is plain Caption text and passes unchanged.
pub fn validate_caption_markup(caption: &str) -> Result<(), String> {
    match scan(caption).error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Returns the Caption text visible to the learner.
///
/// Supported emphasis tags are removed and consecutive whitespace is collapsed
/// to one space. Speech edit indexes are based on this text, not on the raw JSON
/// string that still contains display markup.
pub fn visible_caption_text(caption: &str) -> String {
    collapse_whitespace(&scan(caption).segments)
}

fn collapse_whitespace(segments: &[CaptionSegment]) -> String {
    let visible = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    visible.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Walks the Caption once, producing rendering runs and the first structural
/// error found. Both outputs share one traversal so the rules cannot drift.
fn scan(caption: &str) -> Scan {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut aqua = false;
    let mut open_content_start: Option<usize> = None;
    let mut error: Option<String> = None;
    let mut offset = 0;

    loop {
        let Some(relative_start) = caption[offset..].find('<') else {
            current.push_str(&caption[offset..]);
            break;
        };
        let start = offset + relative_start;
        current.push_str(&caption[offset..start]);

        let Some(relative_end) = caption[start..].find('>') else {
            let rest = &caption[start..];
            let normalized = rest.to_ascii_lowercase();
            if normalized.starts_with("<c.") || normalized.starts_with("</c.") {
                error.get_or_insert_with(|| {
                    format!(
                        "unclosed caption tag at character {}; expected `>`",
                        char_position(caption, start)
                    )
                });
            }
            current.push_str(rest);
            break;
        };
        let end = start + relative_end;
        let token = &caption[start..=end];

        if token[1..].contains('<') {
            // A stray `<` such as `a < b` must not swallow later markup when
            // looking for the closing `>`.
            current.push('<');
            offset = start + 1;
            continue;
        }

        let normalized = token.to_ascii_lowercase();
        let position = char_position(caption, start);
        if normalized == OPEN_TAG {
            if open_content_start.is_some() {
                error.get_or_insert_with(|| {
                    format!("nested `{token}` at character {position}; emphasis spans cannot nest")
                });
            } else {
                push_segment(&mut segments, &mut current, aqua);
                aqua = true;
                open_content_start = Some(end + 1);
            }
        } else if normalized == "</c.aqua>" || normalized == "</c>" {
            let Some(content_start) = open_content_start else {
                error.get_or_insert_with(|| {
                    format!("`{token}` at character {position} has no matching `<c.aqua>`")
                });
                current.push_str(token);
                offset = end + 1;
                continue;
            };
            if caption[content_start..start].trim().is_empty() {
                error.get_or_insert_with(|| {
                    format!(
                        "empty emphasis span at character {position}; remove the tags or add text"
                    )
                });
            }
            push_segment(&mut segments, &mut current, aqua);
            aqua = false;
            open_content_start = None;
        } else if is_caption_tag(&normalized) {
            error.get_or_insert_with(|| {
                format!(
                    "unknown caption tag `{token}` at character {position}; only `<c.aqua>…</c.aqua>` and `<c.aqua>…</c>` are supported"
                )
            });
            current.push_str(token);
        } else {
            current.push_str(token);
        }

        offset = end + 1;
    }

    push_segment(&mut segments, &mut current, aqua);
    if open_content_start.is_some() {
        error.get_or_insert_with(|| {
            "unclosed `<c.aqua>`; close it with `</c.aqua>` or `</c>`".to_string()
        });
    }

    Scan { segments, error }
}

fn push_segment(segments: &mut Vec<CaptionSegment>, current: &mut String, aqua: bool) {
    if current.is_empty() {
        return;
    }
    segments.push(CaptionSegment {
        text: std::mem::take(current),
        aqua,
    });
}

fn char_position(caption: &str, byte_index: usize) -> usize {
    caption[..byte_index].chars().count() + 1
}

fn is_caption_tag(normalized: &str) -> bool {
    normalized.starts_with("<c.")
        || normalized.starts_with("</c.")
        || normalized == "<c>"
        || normalized == "</c>"
}

/// Speakable text for a narration Clip: emphasis markup removed, whitespace
/// collapsed, then `speech_edits` applied to the visible text.
///
/// `speech_edits` indexes are zero-based Unicode character offsets into the
/// visible text. Ranges must match exactly and must not overlap.
pub fn narration_clip_speech_text(clip: &serde_json::Value) -> Result<String, String> {
    let caption = clip
        .get("caption")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "narration Clip must contain a string `caption`".to_string())?;
    let plain = visible_caption_text(caption);
    if plain.is_empty() {
        return Err("narration Clip has no speakable caption text".to_string());
    }
    let edits = match clip.get("speech_edits") {
        None | Some(serde_json::Value::Null) => return Ok(plain),
        Some(value) => value
            .as_array()
            .ok_or_else(|| "narration Clip `speech_edits` must be an array".to_string())?,
    };
    if edits.is_empty() {
        return Ok(plain);
    }

    let characters = plain.chars().collect::<Vec<_>>();
    let mut replacements = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        let object = edit
            .as_object()
            .ok_or_else(|| format!("speech_edits[{index}] must be an object"))?;
        if object.len() != 3
            || !object.contains_key("i")
            || !object.contains_key("s")
            || !object.contains_key("p")
        {
            return Err(format!(
                "speech_edits[{index}] must contain exactly i, s, and p"
            ));
        }
        let start = object["i"]
            .as_u64()
            .ok_or_else(|| format!("speech_edits[{index}].i must be a non-negative integer"))?
            as usize;
        let source = object["s"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("speech_edits[{index}].s must be a non-empty string"))?;
        let replacement = object["p"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("speech_edits[{index}].p must be a non-empty string"))?;
        let end = start + source.chars().count();
        if end > characters.len() {
            return Err(format!(
                "speech_edits[{index}] range is outside the visible Caption"
            ));
        }
        let matched = characters[start..end].iter().collect::<String>();
        if matched != source {
            return Err(format!(
                "speech_edits[{index}].s does not match the visible Caption at index {start}"
            ));
        }
        let start_byte = characters[..start]
            .iter()
            .map(|character| character.len_utf8())
            .sum::<usize>();
        let end_byte = characters[..end]
            .iter()
            .map(|character| character.len_utf8())
            .sum::<usize>();
        replacements.push((start_byte, end_byte, replacement.to_string()));
    }

    replacements.sort_by_key(|(start, _, _)| *start);
    for pair in replacements.windows(2) {
        if pair[1].0 < pair[0].1 {
            return Err("speech_edits ranges overlap".to_string());
        }
    }

    let mut spoken = String::with_capacity(plain.len());
    let mut cursor = 0;
    for (start, end, replacement) in replacements {
        spoken.push_str(&plain[cursor..start]);
        spoken.push_str(&replacement);
        cursor = end;
    }
    spoken.push_str(&plain[cursor..]);
    Ok(spoken)
}

#[cfg(test)]
mod tests;
