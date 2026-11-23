import type {
  Anchor,
  DeleteTextTarget,
  DeleteDirection,
  EditRange,
  InsertTextTarget,
  Position,
} from './applyModel'
import { ApplyError } from './applyModel'
import {
  characterBoundaries,
  findTextMatches,
  lineStartOffset,
  normalizeLineEndings,
  normalizeWithOffsets,
  positionAtOffset,
  type TextMatch,
} from './textMatching'

export type DeleteTargetClassification =
  | { kind: 'complete' }
  | { kind: 'range'; range: TextMatch }
  | { kind: 'invalid'; error: ApplyError }

/** Shared completion and replay check for clean disk and dirty-buffer recovery. */
export function classifyDeleteTarget(
  baseline: string,
  target: DeleteTextTarget,
  direction: DeleteDirection,
): DeleteTargetClassification {
  if (findTextMatches(baseline, target.expected_text).length === 0) {
    return { kind: 'complete' }
  }
  let range: TextMatch & { direction: DeleteDirection }
  try {
    range = resolveDeleteRange(baseline, target, direction)
  } catch (error) {
    if (
      error instanceof ApplyError &&
      ['stale_edit_range', 'line_out_of_range', 'column_out_of_range'].includes(
        error.code,
      )
    ) {
      return { kind: 'complete' }
    }
    return {
      kind: 'invalid',
      error:
        error instanceof ApplyError
          ? error
          : new ApplyError('invalid_delete_target', String(error)),
    }
  }
  if (range.start === range.end) {
    return { kind: 'complete' }
  }
  const expected = normalizeLineEndings(target.expected_text)
  if (
    'cursor' in target &&
    normalizeLineEndings(baseline.slice(range.start, range.end)) !== expected
  ) {
    return { kind: 'complete' }
  }
  const terminal = baseline.slice(0, range.start) + baseline.slice(range.end)
  try {
    const replay = resolveDeleteRange(terminal, target, direction)
    if (replay.start === replay.end) {
      return { kind: 'range', range }
    }
    const replayText = terminal.slice(replay.start, replay.end)
    if ('cursor' in target && normalizeLineEndings(replayText) !== expected) {
      return { kind: 'range', range }
    }
    return {
      kind: 'invalid',
      error: new ApplyError(
        'ambiguous_code_match',
        'DeleteText still matches another occurrence after this deletion; add unique context or use a range',
      ),
    }
  } catch (error) {
    if (error instanceof ApplyError) {
      return { kind: 'range', range }
    }
    throw error
  }
}

export function resolveDeleteRange(
  actual: string,
  target: DeleteTextTarget,
  direction: DeleteDirection,
): TextMatch & { direction: DeleteDirection } {
  if (!('cursor' in target)) {
    return {
      ...resolveEditRange(actual, target, true),
      direction,
    }
  }

  if (!Number.isInteger(target.cursor.count) || target.cursor.count < 1) {
    throw new ApplyError(
      'invalid_delete_count',
      'Deletion count must be a positive integer',
    )
  }
  const resolved = resolveInsertPosition(actual, {
    line: target.cursor.line,
    at: target.cursor.column,
  })
  const boundaries = characterBoundaries(actual)
  const cursorIndex = boundaries.indexOf(resolved.offset)
  if (cursorIndex < 0) {
    throw new ApplyError(
      'invalid_delete_position',
      'The deletion cursor is not on a character boundary',
    )
  }

  if (direction === 'backspace') {
    const startIndex = cursorIndex - target.cursor.count
    if (startIndex < 0) {
      throw new ApplyError(
        'delete_out_of_range',
        `Cannot delete ${target.cursor.count} characters before the cursor`,
      )
    }
    return {
      start: boundaries[startIndex],
      end: resolved.offset,
      direction,
    }
  }

  const endIndex = cursorIndex + target.cursor.count
  if (endIndex >= boundaries.length) {
    throw new ApplyError(
      'delete_out_of_range',
      `Cannot delete ${target.cursor.count} characters after the cursor`,
    )
  }
  return {
    start: resolved.offset,
    end: boundaries[endIndex],
    direction,
  }
}

export function resolveEditRange(
  actual: string,
  target: EditRange,
  allowEmpty = false,
): TextMatch {
  const { start, end } = resolveEditRangeRaw(actual, target, allowEmpty)
  if (start === end) {
    return { start, end }
  }
  const selected = actual.slice(start, end)
  if (
    normalizeLineEndings(selected) !==
    normalizeLineEndings(target.expected_text)
  ) {
    throw new ApplyError(
      'stale_edit_range',
      'selected Edit Range does not match expected text',
    )
  }
  return { start, end }
}

export function resolveEditRangeRaw(
  actual: string,
  target: EditRange,
  allowEmpty = false,
): TextMatch {
  const bounds = rangeBounds(actual, target)
  if (!allowEmpty && bounds.start >= bounds.end) {
    throw new ApplyError(
      'invalid_edit_range',
      'Edit Range start must be before its end',
    )
  }
  return bounds
}

interface NormalizedDocument {
  text: string
  offsets: number[]
}

function rangeBounds(actual: string, target: EditRange): TextMatch {
  const document = normalizeWithOffsets(actual)
  if (target.start.eof) {
    throw new ApplyError(
      'invalid_edit_range',
      'Edit Range start anchor must not be eof',
    )
  }
  const start = anchorOffset(document, target.start, 'start')
  let end = anchorOffset(document, target.end, 'end')
  if (
    end < document.text.length &&
    endIncludesLineBreak(document.text, target.end, end)
  ) {
    end += 1
  }
  if (start > end) {
    throw new ApplyError(
      'invalid_edit_range',
      'Edit Range start must be before its end',
    )
  }
  return {
    start: document.offsets[start]!,
    end: document.offsets[end]!,
  }
}

type AnchorRole = 'start' | 'end' | 'insert'

function anchorOffset(
  document: NormalizedDocument,
  anchor: Anchor,
  role: AnchorRole,
): number {
  if (anchor.eof) {
    if (
      anchor.line !== undefined ||
      anchor.at !== undefined ||
      anchor.placement !== undefined
    ) {
      throw new ApplyError(
        'invalid_anchor',
        'eof Anchor cannot include other fields',
      )
    }
    return document.text.length
  }
  if (anchor.at !== undefined && typeof anchor.at !== 'number') {
    return textAnchorOffset(document, anchor, role)
  }
  if (typeof anchor.at === 'number') {
    if (anchor.line === undefined) {
      throw new ApplyError('invalid_anchor', 'column Anchor requires `line`')
    }
    return columnOffset(
      document.text,
      anchor.line,
      anchor.at,
      effectivePlacement(anchor, role),
    )
  }
  if (anchor.line === undefined) {
    throw new ApplyError(
      'invalid_anchor',
      'Anchor must include `line`, `at`, or `eof`',
    )
  }
  const { start, end } = lineBounds(document.text, anchor.line)
  return effectivePlacement(anchor, role) === 'before' ? start : end
}

export function effectivePlacement(
  anchor: Anchor,
  role: AnchorRole,
): 'before' | 'after' {
  if (anchor.placement !== undefined) {
    return anchor.placement
  }
  if (typeof anchor.at === 'number') {
    return 'before'
  }
  return role === 'end' ? 'after' : 'before'
}

function textAnchorOffset(
  document: NormalizedDocument,
  anchor: Anchor,
  role: AnchorRole,
): number {
  if (anchor.at === undefined || typeof anchor.at === 'number') {
    throw new ApplyError('invalid_anchor', 'text Anchor requires `text`')
  }
  if (anchor.at.text.length === 0) {
    throw new ApplyError('invalid_anchor', 'Anchor text must not be empty')
  }
  const literal = normalizeLineEndings(anchor.at.text)
  const matches = textMatches(document.text, literal, anchor.line)
  const index = anchor.at.index ?? 0
  const start = matches[index]
  if (start === undefined) {
    throw new ApplyError(
      'code_match_missing',
      `Anchor text \`${anchor.at.text}\` index ${index} was not found`,
    )
  }
  return effectivePlacement(anchor, role) === 'before'
    ? start
    : start + literal.length
}

function textMatches(text: string, literal: string, line?: number): number[] {
  if (line !== undefined) {
    const { start, end } = lineBounds(text, line)
    const scope = text.slice(start, end)
    const matches: number[] = []
    let index = scope.indexOf(literal)
    while (index >= 0) {
      matches.push(start + index)
      index = scope.indexOf(literal, index + literal.length)
    }
    return matches
  }
  const matches: number[] = []
  let index = text.indexOf(literal)
  while (index >= 0) {
    matches.push(index)
    index = text.indexOf(literal, index + literal.length)
  }
  return matches
}

function columnOffset(
  text: string,
  line: number,
  column: number,
  placement: 'before' | 'after',
): number {
  const { start, end } = lineBounds(text, line)
  const characters = Array.from(text.slice(start, end))
  if (column > characters.length) {
    throw new ApplyError(
      'column_out_of_range',
      `column ${column} is outside line ${line}`,
    )
  }
  const offset = start + characters.slice(0, column).join('').length
  if (placement === 'before' || column === characters.length) {
    return offset
  }
  return offset + (characters[column]?.length ?? 0)
}

function lineBounds(
  text: string,
  line: number,
): { start: number; end: number } {
  const lines = text.split('\n')
  if (!Number.isInteger(line) || line < 1 || line > lines.length) {
    throw new ApplyError(
      'line_out_of_range',
      `line ${line} is outside the target document`,
    )
  }
  const start = lineStartOffset(lines, line)
  return { start, end: start + lines[line - 1]!.length }
}

/** A range end at a line end always includes the following line break. */
function endIncludesLineBreak(
  text: string,
  anchor: Anchor,
  end: number,
): boolean {
  if (anchor.eof) {
    return false
  }
  if (anchor.at === undefined) {
    return effectivePlacement(anchor, 'end') === 'after'
  }
  if (typeof anchor.at === 'number') {
    if (anchor.line === undefined) {
      return false
    }
    let lineText: string
    try {
      const bounds = lineBounds(text, anchor.line)
      lineText = text.slice(bounds.start, bounds.end)
    } catch {
      return false
    }
    const count = Array.from(lineText).length
    return effectivePlacement(anchor, 'end') === 'before'
      ? anchor.at === count
      : anchor.at + 1 >= count
  }
  return (
    effectivePlacement(anchor, 'end') === 'after' &&
    end > 0 &&
    text[end - 1] !== '\n' &&
    text[end] === '\n'
  )
}

interface ResolvedInsertPosition {
  position: Position
  offset: number
}

export function resolveInsertPosition(
  text: string,
  target: InsertTextTarget,
): ResolvedInsertPosition {
  const document = normalizeWithOffsets(text)
  const offset = document.offsets[anchorOffset(document, target, 'insert')]!
  return {
    position: positionAtOffset(text, offset),
    offset,
  }
}
