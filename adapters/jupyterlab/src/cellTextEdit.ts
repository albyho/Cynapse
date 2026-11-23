export type CellPlacement = 'before' | 'after'

export interface CellTextAnchor {
  text: string
  index?: number
}

export type CellAnchorAt = number | CellTextAnchor

export interface CellAnchor {
  line?: number
  at?: CellAnchorAt
  placement?: CellPlacement
  eof?: boolean
}

export type CellInsertTarget = CellAnchor

export interface CellEditRange {
  start: CellAnchor
  end: CellAnchor
  expected_text: string
}

export interface CellDeleteCursorTarget {
  cursor: {
    line: number
    column: number
    count: number
  }
  expected_text: string
}

export type CellDeleteTarget = CellEditRange | CellDeleteCursorTarget

export interface CellEditPlan {
  next: string
  start: number
  end: number
  text: string
}

export class CellTextEditError extends Error {
  readonly code = 'cell_text_target_invalid'

  constructor(message: string) {
    super(message)
    this.name = 'CellTextEditError'
  }
}

type AnchorRole = 'start' | 'end' | 'insert'

export function normalizeLineEndings(text: string): string {
  return text.replace(/\r\n?/g, '\n')
}

export type CellLineEnding = '\n' | '\r\n'

/** Use CRLF when it is at least half of the cell's line breaks. */
export function cellLineEnding(source: string): CellLineEnding {
  const lineBreaks = source.split('\n').length - 1
  const crlf = source.split('\r\n').length - 1
  return crlf > 0 && crlf * 2 >= lineBreaks ? '\r\n' : '\n'
}

export function withLineEnding(
  text: string,
  lineEnding: CellLineEnding,
): string {
  return lineEnding === '\n' ? text : text.replace(/\n/g, '\r\n')
}

export function planCellInsertText(
  source: string,
  target: CellInsertTarget,
  text: string,
): CellEditPlan {
  const document = normalizeLineEndings(source)
  const offset = anchorOffset(document, target, 'insert')
  const code = normalizeLineEndings(text)
  return {
    next: `${document.slice(0, offset)}${code}${document.slice(offset)}`,
    start: offset,
    end: offset,
    text: code,
  }
}

export function planCellDeleteText(
  source: string,
  target: CellDeleteTarget,
  direction: 'backspace' | 'delete' = 'backspace',
): CellEditPlan {
  const document = normalizeLineEndings(source)
  const [start, end] = deleteOffsets(document, target, direction)
  return {
    next: `${document.slice(0, start)}${document.slice(end)}`,
    start,
    end,
    text: '',
  }
}

export function planCellReplaceText(
  source: string,
  target: CellEditRange,
  text: string,
): CellEditPlan {
  const document = normalizeLineEndings(source)
  const [start, end] = editRangeOffsets(document, target)
  const code = normalizeLineEndings(text)
  return {
    next: `${document.slice(0, start)}${code}${document.slice(end)}`,
    start,
    end,
    text: code,
  }
}

function effectivePlacement(
  anchor: CellAnchor,
  role: AnchorRole,
): CellPlacement {
  if (anchor.placement) {
    return anchor.placement
  }
  if (typeof anchor.at === 'number') {
    return 'before'
  }
  return role === 'end' ? 'after' : 'before'
}

function anchorOffset(
  document: string,
  anchor: CellAnchor,
  role: AnchorRole,
): number {
  if (anchor.eof) {
    if (role === 'start') {
      throw new CellTextEditError('start anchor must not be eof')
    }
    return document.length
  }
  const placement = effectivePlacement(anchor, role)
  if (typeof anchor.at === 'number') {
    if (anchor.line === undefined) {
      throw new CellTextEditError('column anchor requires `line`')
    }
    return columnOffset(document, anchor.line, anchor.at, placement)
  }
  if (anchor.at) {
    return textAnchorOffset(document, anchor.at, anchor.line, placement)
  }
  if (anchor.line === undefined) {
    throw new CellTextEditError('anchor requires `line`, `at`, or `eof`')
  }
  const [start, end] = lineBounds(document, anchor.line)
  return placement === 'before' ? start : end
}

function columnOffset(
  document: string,
  line: number,
  column: number,
  placement: CellPlacement,
): number {
  const [start, , lineText] = lineBounds(document, line)
  const characters = Array.from(lineText)
  if (column > characters.length) {
    throw new CellTextEditError(`column ${column} is outside line ${line}`)
  }
  const offset = start + characters.slice(0, column).join('').length
  if (placement === 'before' || column === characters.length) {
    return offset
  }
  return offset + characters[column].length
}

function textAnchorOffset(
  document: string,
  anchor: CellTextAnchor,
  line: number | undefined,
  placement: CellPlacement,
): number {
  const matches = literalMatches(document, anchor.text, line)
  const index = anchor.index ?? 0
  const offset = matches[index]
  if (offset === undefined) {
    throw new CellTextEditError(
      `anchor text "${anchor.text}" index ${index} was not found`,
    )
  }
  return placement === 'before' ? offset : offset + anchor.text.length
}

function literalMatches(
  document: string,
  literal: string,
  line?: number,
): number[] {
  if (literal.length === 0) {
    throw new CellTextEditError('anchor text must not be empty')
  }
  if (line === undefined) {
    return indicesOf(document, literal, 0)
  }
  const [start, end] = lineBounds(document, line)
  return indicesOf(document.slice(start, end), literal, start)
}

function indicesOf(text: string, literal: string, base: number): number[] {
  const matches: number[] = []
  let offset = text.indexOf(literal)
  while (offset >= 0) {
    matches.push(base + offset)
    offset = text.indexOf(literal, offset + literal.length)
  }
  return matches
}

function deleteOffsets(
  document: string,
  target: CellDeleteTarget,
  direction: 'backspace' | 'delete',
): [number, number] {
  if ('cursor' in target) {
    const cursor = textPositionOffset(
      document,
      target.cursor.line,
      target.cursor.column,
    )
    const offsets = [0]
    for (const character of document) {
      offsets.push(offsets[offsets.length - 1] + character.length)
    }
    const cursorIndex = offsets.indexOf(cursor)
    if (cursorIndex < 0) {
      throw new CellTextEditError('DeleteText cursor is invalid')
    }
    const count = target.cursor.count
    const startIndex =
      direction === 'backspace' ? cursorIndex - count : cursorIndex
    const endIndex =
      direction === 'backspace' ? cursorIndex : cursorIndex + count
    if (startIndex < 0 || endIndex >= offsets.length) {
      throw new CellTextEditError(
        `DeleteText cursor has fewer than ${count} characters`,
      )
    }
    const start = offsets[startIndex]
    const end = offsets[endIndex]
    if (
      document.slice(start, end) !== normalizeLineEndings(target.expected_text)
    ) {
      throw new CellTextEditError(
        'DeleteText cursor does not match expected_text',
      )
    }
    return [start, end]
  }
  return editRangeOffsets(document, target)
}

function editRangeOffsets(
  document: string,
  range: CellEditRange,
): [number, number] {
  const start = anchorOffset(document, range.start, 'start')
  let end = anchorOffset(document, range.end, 'end')
  if (end < document.length && endIncludesLineBreak(document, range.end, end)) {
    end += 1
  }
  if (start >= end) {
    throw new CellTextEditError('Edit Range start must be before its end')
  }
  if (
    document.slice(start, end) !== normalizeLineEndings(range.expected_text)
  ) {
    throw new CellTextEditError('Edit Range does not match expected_text')
  }
  return [start, end]
}

/** A range end at a line end includes the following line break by default. */
function endIncludesLineBreak(
  document: string,
  anchor: CellAnchor,
  end: number,
): boolean {
  if (anchor.eof) {
    return false
  }
  if (typeof anchor.at === 'number') {
    if (anchor.line === undefined) {
      return false
    }
    const [, , lineText] = lineBounds(document, anchor.line)
    const count = Array.from(lineText).length
    return effectivePlacement(anchor, 'end') === 'before'
      ? anchor.at === count
      : anchor.at + 1 >= count
  }
  if (anchor.at) {
    return (
      effectivePlacement(anchor, 'end') === 'after' &&
      end > 0 &&
      !document.slice(0, end).endsWith('\n') &&
      document.slice(end).startsWith('\n')
    )
  }
  return effectivePlacement(anchor, 'end') === 'after'
}

function lineBounds(document: string, line: number): [number, number, string] {
  if (
    !Number.isInteger(line) ||
    line < 1 ||
    line > document.split('\n').length
  ) {
    throw new CellTextEditError(`line ${line} is outside the document`)
  }
  const start = lineStartOffset(document, line)
  const lineBreak = document.indexOf('\n', start)
  const end = lineBreak < 0 ? document.length : lineBreak
  return [start, end, document.slice(start, end)]
}

function textPositionOffset(
  document: string,
  line: number,
  column: number,
): number {
  const [start, , lineText] = lineBounds(document, line)
  const characters = Array.from(lineText)
  if (column > characters.length) {
    throw new CellTextEditError(`column ${column} is outside line ${line}`)
  }
  return start + characters.slice(0, column).join('').length
}

function lineStartOffset(document: string, line: number): number {
  let offset = 0
  for (let index = 1; index < line; index += 1) {
    offset = document.indexOf('\n', offset) + 1
  }
  return offset
}
