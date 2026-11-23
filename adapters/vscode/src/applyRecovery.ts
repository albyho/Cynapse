import type {
  ApplyOutcome,
  DeleteTextAction,
  DeleteTextTarget,
  DeleteDirection,
  EditRange,
  EditorFacade,
  InsertTextAction,
  InsertTextTarget,
  PreparedTarget,
  ReplaceTextAction,
} from './applyModel'
import { ApplyError } from './applyModel'
import {
  findTextMatches,
  matchTextPrefix,
  normalizeLineEndings,
  normalizeWithOffsets,
  positionAtOffset,
  suffixPrefixLength,
  type TextMatch,
} from './textMatching'
import {
  classifyDeleteTarget,
  effectivePlacement,
  resolveEditRangeRaw,
  resolveInsertPosition,
} from './editResolution'

// ---------------------------------------------------------------------------
// Recovery self-heal (ADR-0039). Editing Actions only save on success, so the
// disk file is the pre-Action baseline. A crashed run leaves its own partial
// product in a hot-exit dirty buffer. On retry the buffer-vs-disk diff must be
// exactly a partial application of THIS Action (nothing typed, partially
// applied, or fully applied but unsaved); recovery then converges the document
// to the Action terminal state in one instant edit with no typewriter
// animation. Any other diff is user content and is rejected untouched.
// ---------------------------------------------------------------------------

function dirtyBaseline(target: PreparedTarget): string {
  return normalizeLineEndings(target.savedText)
}

// User content in the buffer makes self-heal impossible. The failure must
// name the file and the learner's options (issue #74, user story 8).
function dirtyRejection(path: string): ApplyError {
  return new ApplyError(
    'document_dirty',
    `"${path}" has unsaved changes that are not a partial result of this action. Undo your changes to the file or run an explicit Unit Reset.`,
  )
}

function bufferOffset(currentText: string, normalizedIndex: number): number {
  const offset = normalizeWithOffsets(currentText).offsets[normalizedIndex]
  if (offset === undefined) {
    throw new ApplyError(
      'recovery_offset_invalid',
      'The crashed buffer no longer matches the recorded edit position',
    )
  }
  return offset
}

function documentEol(text: string): string {
  return text.includes('\r\n') ? '\r\n' : '\n'
}

export async function recoverInsertText(
  action: InsertTextAction,
  target: PreparedTarget,
  currentText: string,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const baseline = dirtyBaseline(target)
  const buffer = normalizeLineEndings(currentText)
  const code = normalizeLineEndings(action.text)
  const state = detectInsertRecovery(baseline, buffer, action.target, code)
  if (!state) {
    throw dirtyRejection(action.path)
  }

  // The disk already holds this Action's terminal product: a later Action in
  // the replayed Step crashed mid-edit and left its own partial in the
  // buffer. Skip without saving so that Action can heal it (ADR-0039).
  if (state.kind === 'complete') {
    return 'done'
  }

  const cursorRaw = bufferOffset(currentText, state.offset + state.matched)
  const cursor = positionAtOffset(currentText, cursorRaw)
  if (state.matched >= state.remainder.length) {
    await facade.select(target, cursor, cursor)
    await facade.save(target)
    return 'done'
  }
  if (signal?.aborted) {
    return 'cancelled'
  }

  const remaining = state.remainder
    .slice(state.matched)
    .replace(/\r\n?|\n/g, documentEol(currentText))
  await facade.insert(target, cursor, remaining, {
    undoStopBefore: true,
    undoStopAfter: true,
  })
  await facade.save(target)
  return 'done'
}

type InsertRecovery =
  | { kind: 'complete' }
  | {
      kind: 'partial'
      offset: number
      matched: number
      remainder: string
    }

/**
 * The first run trims an overlap for EOF and `literal before`; recovery must
 * compare against the same remainder, not the full Action text.
 */
function insertRemainder(
  baseline: string,
  offset: number,
  target: InsertTextTarget,
  code: string,
): { consumed: number; remainder: string } {
  let consumed: number
  if (target.eof) {
    consumed = suffixPrefixLength(baseline, code)
  } else if (
    target.at !== undefined &&
    typeof target.at !== 'number' &&
    effectivePlacement(target, 'insert') === 'before'
  ) {
    consumed = suffixPrefixLength(baseline.slice(0, offset), code)
  } else {
    consumed = matchTextPrefix(baseline.slice(offset), code).matched
  }
  return { consumed, remainder: code.slice(consumed) }
}

function detectInsertRecovery(
  baseline: string,
  buffer: string,
  target: InsertTextTarget,
  code: string,
): InsertRecovery | undefined {
  let offset: number
  try {
    offset = resolveInsertPosition(baseline, target).offset
  } catch {
    return undefined
  }
  const { consumed, remainder } = insertRemainder(
    baseline,
    offset,
    target,
    code,
  )
  // The disk baseline already contains the complete product: this Action
  // completed and saved before a later Action crashed.
  if (consumed >= code.length) {
    return { kind: 'complete' }
  }
  if (!buffer.startsWith(baseline.slice(0, offset))) {
    return undefined
  }
  const maxMatched = matchTextPrefix(buffer.slice(offset), remainder).matched
  for (let matched = maxMatched; matched >= 0; matched -= 1) {
    if (buffer.slice(offset + matched) === baseline.slice(offset)) {
      return { kind: 'partial', offset, matched, remainder }
    }
  }
  return undefined
}

export async function recoverDeleteText(
  action: DeleteTextAction,
  target: PreparedTarget,
  currentText: string,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const baseline = dirtyBaseline(target)
  const buffer = normalizeLineEndings(currentText)
  const direction = action.direction ?? 'backspace'
  const remaining = detectDeleteRecovery(
    baseline,
    buffer,
    action.target,
    direction,
  )
  if (!remaining) {
    throw dirtyRejection(action.path)
  }
  if (remaining.kind === 'complete') {
    // Already deleted on disk; the dirty diff belongs to a later Action.
    return 'done'
  }
  if (remaining.start < remaining.end) {
    if (signal?.aborted) {
      return 'cancelled'
    }
    await facade.replace(
      target,
      positionAtOffset(currentText, bufferOffset(currentText, remaining.start)),
      positionAtOffset(currentText, bufferOffset(currentText, remaining.end)),
      '',
      { undoStopBefore: true, undoStopAfter: true },
    )
  }
  await facade.save(target)
  return 'done'
}

type DeleteRecovery =
  { kind: 'complete' } | { kind: 'remaining'; start: number; end: number }

function detectDeleteRecovery(
  baseline: string,
  buffer: string,
  target: DeleteTextTarget,
  direction: DeleteDirection,
): DeleteRecovery | undefined {
  const baselineMatches = findTextMatches(baseline, target.expected_text).length
  const bufferMatches = findTextMatches(buffer, target.expected_text).length
  if (baselineMatches === 0) {
    // The disk already holds the post-delete text; a later Action owns the
    // dirty buffer diff. Requiring the expected text to be absent from the
    // buffer too keeps unrelated user content from being mistaken for that
    // later partial.
    return bufferMatches === 0 ? { kind: 'complete' } : undefined
  }
  const classification = classifyDeleteTarget(baseline, target, direction)
  if (classification.kind === 'complete') {
    return { kind: 'complete' }
  }
  if (classification.kind === 'invalid') {
    return undefined
  }
  const { start, end } = classification.range

  if (direction === 'backspace') {
    // Partial backspace removes from the range end backwards, so the buffer
    // is baseline[0..u] + baseline[end..] with u in [start..end].
    const tail = baseline.slice(end)
    if (!buffer.endsWith(tail)) {
      return undefined
    }
    const head = buffer.slice(0, buffer.length - tail.length)
    if (head.length < start || head.length > end) {
      return undefined
    }
    if (!baseline.startsWith(head)) {
      return undefined
    }
    return { kind: 'remaining', start, end: head.length }
  }

  // Forward deletion removes from the range start onwards, so the buffer is
  // baseline[0..start] + baseline[v..] with v in [start..end].
  const head = baseline.slice(0, start)
  if (!buffer.startsWith(head)) {
    return undefined
  }
  const tail = buffer.slice(start)
  const v = baseline.length - tail.length
  if (v < start || v > end) {
    return undefined
  }
  if (baseline.slice(v) !== tail) {
    return undefined
  }
  return { kind: 'remaining', start, end: start + (end - v) }
}

export async function recoverReplaceText(
  action: ReplaceTextAction,
  target: PreparedTarget,
  currentText: string,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const baseline = dirtyBaseline(target)
  const buffer = normalizeLineEndings(currentText)
  const code = normalizeLineEndings(action.text)
  const state = detectReplaceRecovery(baseline, buffer, action.target, code)
  if (!state) {
    throw dirtyRejection(action.path)
  }
  // The disk already holds the replacement: a later Action in the replayed
  // Step owns the dirty diff. Skip without saving (ADR-0039).
  if (state.kind === 'complete') {
    return 'done'
  }
  if (state.middle === code.length) {
    await facade.save(target)
    return 'done'
  }
  if (signal?.aborted) {
    return 'cancelled'
  }

  const replacement = action.text.replace(/\r\n?|\n/g, documentEol(currentText))
  await facade.replace(
    target,
    positionAtOffset(currentText, bufferOffset(currentText, state.start)),
    positionAtOffset(
      currentText,
      bufferOffset(currentText, state.start + state.middle),
    ),
    replacement,
    { undoStopBefore: true, undoStopAfter: true },
  )
  await facade.save(target)
  return 'done'
}

type ReplaceRecovery =
  { kind: 'complete' } | { kind: 'partial'; start: number; middle: number }

function detectReplaceRecovery(
  baseline: string,
  buffer: string,
  target: EditRange,
  code: string,
): ReplaceRecovery | undefined {
  let selection: TextMatch
  try {
    selection = resolveEditRangeRaw(baseline, target)
  } catch {
    return undefined
  }
  const expected = normalizeLineEndings(
    baseline.slice(selection.start, selection.end),
  )
  if (expected === code) {
    return { kind: 'complete' }
  }
  if (expected !== normalizeLineEndings(target.expected_text)) {
    return undefined
  }
  if (!buffer.startsWith(baseline.slice(0, selection.start))) {
    return undefined
  }
  if (!buffer.endsWith(baseline.slice(selection.end))) {
    return undefined
  }
  const tailLength = baseline.length - selection.end
  const middle = buffer.length - selection.start - tailLength
  if (middle < 0) {
    return undefined
  }
  const remnant = buffer.slice(selection.start, selection.start + middle)
  const isDeletionRemnant = expected.startsWith(remnant)
  const isReplacementPrefix = code.startsWith(remnant)
  if (!isDeletionRemnant && !isReplacementPrefix) {
    return undefined
  }
  return { kind: 'partial', start: selection.start, middle }
}
