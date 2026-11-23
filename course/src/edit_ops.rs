//! Pure, in-memory semantics for code-editing Actions.
//!
//! The Client scenario harness and the runtime must agree on what `InsertText`,
//! `DeleteText` and `ReplaceText` do. Only the surrounding file I/O differs.

use std::fmt;

use crate::{
    Anchor, AnchorAt, DeleteDirection, DeleteTextAction, DeleteTextTarget, EditRange,
    InsertTextAction, Placement, ReplaceTextAction,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOpError(pub String);

impl fmt::Display for EditOpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for EditOpError {}

type Result<T> = std::result::Result<T, EditOpError>;

fn protocol(message: impl Into<String>) -> EditOpError {
    EditOpError(message.into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Start,
    End,
    Insert,
}

pub fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
pub fn apply_insert_text(text: &str, action: &InsertTextAction) -> Result<String> {
    let normalized = normalize_line_endings(text);
    let target = &action.target;
    let mut insert_offset = insert_anchor_offset(&normalized, target)
        .map_err(|error| protocol(format!("InsertText {} (in {})", error.0, action.path)))?;

    let code = normalize_line_endings(&action.text);
    let remaining = if target.eof {
        if normalized.ends_with(&code) {
            return Ok(normalized);
        }
        &code[suffix_prefix_len(&normalized, &code)..]
    } else if matches!(&target.at, Some(AnchorAt::Text(_)))
        && target.placement_or_default(false) == Placement::Before
    {
        let consumed = suffix_prefix_len(&normalized[..insert_offset], &code);
        if consumed >= code.len() {
            return Ok(normalized);
        }
        &code[consumed..]
    } else {
        let consumed = prefix_len(&normalized[insert_offset..], &code);
        insert_offset += consumed;
        &code[consumed..]
    };
    let mut updated = normalized;
    updated.insert_str(insert_offset, remaining);
    Ok(updated)
}

pub fn apply_delete_text(text: &str, action: &DeleteTextAction) -> Result<String> {
    let current = normalize_line_endings(text);
    let (start, end) = match &action.target {
        DeleteTextTarget::Range(range) => match edit_range_offsets(&current, range) {
            Ok(offsets) => offsets,
            Err(error) => recover_delete_range_offsets(&current, range).ok_or(error)?,
        },
        DeleteTextTarget::Cursor(target) => {
            let cursor = text_position_offset(&current, target.cursor.line, target.cursor.column)?;
            let characters = current
                .char_indices()
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            let unit_count = target.cursor.count as usize;
            let cursor_index = if cursor == current.len() {
                characters.len()
            } else {
                characters
                    .iter()
                    .position(|offset| *offset == cursor)
                    .ok_or_else(|| protocol("DeleteText cursor is invalid"))?
            };
            let (start, end) = match action.direction.unwrap_or(DeleteDirection::Backspace) {
                DeleteDirection::Backspace => {
                    let start_index = cursor_index.checked_sub(unit_count).ok_or_else(|| {
                        protocol(format!(
                            "DeleteText has fewer than {count} characters before the cursor",
                            count = target.cursor.count
                        ))
                    })?;
                    let start = characters[start_index];
                    (start, cursor)
                }
                DeleteDirection::Delete => {
                    if cursor_index + unit_count > characters.len() {
                        return Err(protocol(format!(
                            "DeleteText has fewer than {count} characters after the cursor",
                            count = target.cursor.count
                        )));
                    }
                    let end = characters
                        .get(cursor_index + unit_count)
                        .copied()
                        .unwrap_or(current.len());
                    (cursor, end)
                }
            };
            let expected = normalize_line_endings(&target.expected_text);
            if current.get(start..end) != Some(expected.as_str()) {
                return Err(protocol("DeleteText cursor does not match expected_text"));
            }
            (start, end)
        }
    };
    let mut updated = current;
    updated.replace_range(start..end, "");
    Ok(updated)
}

pub fn apply_replace_text(text: &str, action: &ReplaceTextAction) -> Result<String> {
    let current = normalize_line_endings(text);
    let code = normalize_line_endings(&action.text);
    let (start, end) = match edit_range_offsets(&current, &action.target) {
        Ok(offsets) => offsets,
        Err(error) => {
            if let Some(offsets) = recover_replace_range_offsets(&current, &action.target, &code) {
                offsets
            } else if missing_text_anchor(&current, &action.target)
                && is_completed_replace(&current, action)
            {
                return Ok(current);
            } else {
                return Err(error);
            }
        }
    };
    if current[start..end] == code {
        return Ok(current);
    }
    let mut updated = current;
    updated.replace_range(start..end, &code);
    Ok(updated)
}

/// A completed replacement can outlive its text anchors: the replacement is
/// uniquely present and the old range is gone (ADR-0039). Coordinate, ambiguous
/// and malformed ranges still fail.
fn missing_text_anchor(text: &str, target: &EditRange) -> bool {
    [&target.start, &target.end].iter().any(|anchor| {
        let Some(AnchorAt::Text(literal)) = &anchor.at else {
            return false;
        };
        let Ok(matches) = text_matches(text, &literal.text, anchor.line) else {
            return true;
        };
        let count = matches.len();
        count == 0 || literal.index.is_some_and(|index| index as usize >= count)
    })
}

fn is_completed_replace(text: &str, action: &ReplaceTextAction) -> bool {
    let code = normalize_line_endings(&action.text);
    let expected = normalize_line_endings(&action.target.expected_text);
    !code.is_empty()
        && !expected.is_empty()
        && text.match_indices(&code).count() == 1
        && text.match_indices(&expected).count() == 0
}

/// Offsets for a replayed Edit Range deletion: adjacent anchors mean the range
/// is already gone, and a proper prefix or suffix remnant of the expected text
/// means an interrupted deletion that must finish.
pub fn recover_delete_range_offsets(text: &str, range: &EditRange) -> Option<(usize, usize)> {
    let (start, end) = range_bounds(text, range).ok()?;
    if start == end {
        return Some((start, end));
    }
    let expected = normalize_line_endings(&range.expected_text);
    let middle = text.get(start..end)?;
    if middle != expected && (expected.starts_with(middle) || expected.ends_with(middle)) {
        Some((start, end))
    } else {
        None
    }
}

/// Offsets for a replayed replacement: the range holds a proper remnant of the
/// expected text or a prefix of the replacement code.
pub fn recover_replace_range_offsets(
    text: &str,
    range: &EditRange,
    code: &str,
) -> Option<(usize, usize)> {
    let (start, end) = range_bounds(text, range).ok()?;
    let expected = normalize_line_endings(&range.expected_text);
    let middle = text.get(start..end)?;
    if middle != expected && (expected.starts_with(middle) || code.starts_with(middle)) {
        Some((start, end))
    } else {
        None
    }
}

pub fn edit_range_offsets(text: &str, range: &EditRange) -> Result<(usize, usize)> {
    let (start, end) = range_bounds(text, range)?;
    if start >= end {
        return Err(protocol("Edit Range start must be before its end"));
    }
    let expected = normalize_line_endings(&range.expected_text);
    if text[start..end] != expected {
        return Err(protocol("Edit Range does not match expected_text"));
    }
    Ok((start, end))
}

fn range_bounds(text: &str, range: &EditRange) -> Result<(usize, usize)> {
    let start = range_anchor_offset(text, &range.start, "start")?;
    let mut end = range_anchor_offset(text, &range.end, "end")?;
    if end < text.len() && end_includes_line_break(text, &range.end, end) {
        end += 1;
    }
    if start > end {
        return Err(protocol("Edit Range start must be before its end"));
    }
    Ok((start, end))
}

pub fn range_anchor_offset(text: &str, anchor: &Anchor, role: &str) -> Result<usize> {
    let role = match role {
        "start" => Role::Start,
        "end" => Role::End,
        other => return Err(protocol(format!("unknown Anchor role `{other}`"))),
    };
    anchor_offset(text, anchor, role)
}

fn insert_anchor_offset(text: &str, anchor: &Anchor) -> Result<usize> {
    anchor_offset(text, anchor, Role::Insert)
}

fn anchor_offset(text: &str, anchor: &Anchor, role: Role) -> Result<usize> {
    anchor.validate().map_err(protocol)?;
    if anchor.eof {
        return Ok(text.len());
    }
    match &anchor.at {
        Some(AnchorAt::Text(literal)) => text_anchor_offset(
            text,
            &literal.text,
            literal.index,
            anchor.line,
            anchor,
            role,
        ),
        Some(AnchorAt::Column(column)) => {
            let line = anchor
                .line
                .ok_or_else(|| protocol("column Anchor requires `line`"))?;
            column_offset(
                text,
                line,
                *column,
                anchor.placement_or_default(role == Role::End),
            )
        }
        None => {
            let line = anchor
                .line
                .ok_or_else(|| protocol("line Anchor requires `line`"))?;
            let (start, end, _) = line_bounds(text, line)?;
            match anchor.placement_or_default(role == Role::End) {
                Placement::Before => Ok(start),
                Placement::After => Ok(end),
            }
        }
    }
}

fn text_anchor_offset(
    text: &str,
    literal: &str,
    index: Option<u32>,
    line: Option<u32>,
    anchor: &Anchor,
    role: Role,
) -> Result<usize> {
    let matches = text_matches(text, literal, line)?;
    let index = index.unwrap_or(0) as usize;
    let start = *matches.get(index).ok_or_else(|| {
        protocol(format!(
            "Anchor text `{literal}` index {index} was not found"
        ))
    })?;
    Ok(match anchor.placement_or_default(role == Role::End) {
        Placement::Before => start,
        Placement::After => start + literal.len(),
    })
}

fn text_matches(text: &str, literal: &str, line: Option<u32>) -> Result<Vec<usize>> {
    if literal.is_empty() {
        return Err(protocol("Anchor text must not be empty"));
    }
    if let Some(line) = line {
        let (start, end, _) = line_bounds(text, line)?;
        Ok(text[start..end]
            .match_indices(literal)
            .map(|(offset, _)| start + offset)
            .collect())
    } else {
        Ok(text
            .match_indices(literal)
            .map(|(offset, _)| offset)
            .collect())
    }
}

fn column_offset(text: &str, line: u32, column: u32, placement: Placement) -> Result<usize> {
    let (start, _, line_text) = line_bounds(text, line)?;
    let count = line_text.chars().count();
    if column as usize > count {
        return Err(protocol(format!("column {column} is outside line {line}")));
    }
    let offset = start
        + line_text
            .chars()
            .take(column as usize)
            .map(char::len_utf8)
            .sum::<usize>();
    if placement == Placement::Before || column as usize == count {
        return Ok(offset);
    }
    let char_len = line_text
        .chars()
        .nth(column as usize)
        .map(char::len_utf8)
        .unwrap_or(0);
    Ok(offset + char_len)
}

fn line_bounds(text: &str, line: u32) -> Result<(usize, usize, &str)> {
    if line == 0 || line as usize > text.split('\n').count() {
        return Err(protocol(format!("line {line} is outside the document")));
    }
    let start = line_start_offset(text, line as usize);
    let length = text[start..].find('\n').unwrap_or(text.len() - start);
    Ok((start, start + length, &text[start..start + length]))
}

/// A range end at a line end includes the following line break by default.
fn end_includes_line_break(text: &str, anchor: &Anchor, end: usize) -> bool {
    if anchor.eof {
        return false;
    }
    match &anchor.at {
        None => anchor.placement_or_default(true) == Placement::After,
        Some(AnchorAt::Column(column)) => {
            let Some(line) = anchor.line else {
                return false;
            };
            let Ok((_, _, line_text)) = line_bounds(text, line) else {
                return false;
            };
            let count = line_text.chars().count();
            match anchor.placement_or_default(true) {
                Placement::Before => *column as usize == count,
                Placement::After => *column as usize + 1 >= count,
            }
        }
        Some(AnchorAt::Text(_)) => {
            anchor.placement_or_default(true) == Placement::After
                && end > 0
                && !text[..end].ends_with('\n')
                && text[end..].starts_with('\n')
        }
    }
}

pub fn text_position_offset(text: &str, line: u32, column: u32) -> Result<usize> {
    let (start, _, line_text) = line_bounds(text, line)?;
    if column as usize > line_text.chars().count() {
        return Err(protocol(format!("column {column} is outside line {line}")));
    }
    Ok(start
        + line_text
            .chars()
            .take(column as usize)
            .map(char::len_utf8)
            .sum::<usize>())
}

pub fn line_start_offset(text: &str, line: usize) -> usize {
    text.split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum()
}

pub fn prefix_len(actual: &str, expected: &str) -> usize {
    actual
        .chars()
        .zip(expected.chars())
        .take_while(|(actual, expected)| actual == expected)
        .map(|(character, _)| character.len_utf8())
        .sum()
}

pub fn suffix_prefix_len(actual: &str, expected: &str) -> usize {
    let expected_chars = expected.chars().collect::<Vec<_>>();
    if expected_chars.is_empty() {
        return 0;
    }

    // KMP prefix table for the expected text. Scan the actual text while
    // tracking the longest expected prefix that ends at the current character.
    let mut prefix = vec![0_usize; expected_chars.len()];
    for index in 1..expected_chars.len() {
        let mut length = prefix[index - 1];
        while length > 0 && expected_chars[index] != expected_chars[length] {
            length = prefix[length - 1];
        }
        if expected_chars[index] == expected_chars[length] {
            length += 1;
        }
        prefix[index] = length;
    }

    let actual_chars = actual.chars().collect::<Vec<_>>();
    let mut matched = 0_usize;
    for (index, character) in actual_chars.iter().enumerate() {
        while matched > 0 && *character != expected_chars[matched] {
            matched = prefix[matched - 1];
        }
        if *character == expected_chars[matched] {
            matched += 1;
        }
        // A full match is only the answer when it ends at the end of actual.
        // Otherwise continue from its border to find the suffix at the end.
        if matched == expected_chars.len() && index + 1 < actual_chars.len() {
            matched = prefix[matched - 1];
        }
    }

    expected_chars[..matched]
        .iter()
        .map(|character| character.len_utf8())
        .sum()
}
