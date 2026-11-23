import type {
  ApplyOutcome,
  ClearRevealAction,
  CloseFileAction,
  ClosePreviewAction,
  DeleteTextAction,
  EditorFacade,
  InsertTextAction,
  OpenFileAction,
  OpenPreviewAction,
  OpenFileWithPreviewAction,
  Position,
  PreparedTarget,
  ReplaceTextAction,
  RevealTextAction,
} from './applyModel'
import { ApplyError } from './applyModel'
import {
  advancePosition,
  findTextMatches,
  normalizeLineEndings,
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
import {
  recoverDeleteText,
  recoverInsertText,
  recoverReplaceText,
} from './applyRecovery'
import { runChunkedEdit } from './chunkedEdit'
import { splitIntoTypewriterChunks } from '@cynapse/adapter-kit'

export * from './applyModel'
export { advancePosition, matchTextPrefix } from './textMatching'
export { resolveInsertPosition } from './editResolution'

export async function applyOpenFile(
  action: OpenFileAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  await facade.closeSidePreviews()
  await facade.open(action.path, 'first')
  return signal?.aborted ? 'cancelled' : 'done'
}

export async function applyOpenPreview(
  action: OpenPreviewAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  await facade.preview(action.path, action.view)
  return signal?.aborted ? 'cancelled' : 'done'
}

export async function applyOpenFileWithPreview(
  action: OpenFileWithPreviewAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  await facade.closeSidePreviews()
  await facade.open(action.path, 'first')
  if (signal?.aborted) {
    return 'cancelled'
  }
  await facade.preview(action.path, 'side')
  return signal?.aborted ? 'cancelled' : 'done'
}

export async function applyCloseFile(
  action: CloseFileAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  await facade.closeFile(action.path)
  return signal?.aborted ? 'cancelled' : 'done'
}

export async function applyClosePreview(
  action: ClosePreviewAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  await facade.closePreview(action.path)
  return signal?.aborted ? 'cancelled' : 'done'
}

export async function applyRevealText(
  action: RevealTextAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  const target = await facade.open(action.path, 'first')
  if (signal?.aborted) {
    return 'cancelled'
  }
  const start = {
    line: action.start.line - 1,
    character: action.start.column ?? 0,
  }
  const end = action.end
    ? { line: action.end.line - 1, character: action.end.column ?? 0 }
    : start
  await facade.reveal(target, start, end)
  return signal?.aborted ? 'cancelled' : 'done'
}

export async function applyClearReveal(
  _action: ClearRevealAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  await facade.clearReveal()
  return signal?.aborted ? 'cancelled' : 'done'
}

export async function applyInsertText(
  action: InsertTextAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const speedMs = action.speed_ms ?? 0
  validateTextEditSpeed('Insertion', speedMs)
  if (action.text.length === 0) {
    throw new ApplyError('invalid_code', 'Inserted code must not be empty')
  }
  if (signal?.aborted) {
    return 'cancelled'
  }

  const target = await facade.existingTarget(action.path)
  const currentText = await facade.text(target)
  if (target.dirty) {
    return recoverInsertText(action, target, currentText, facade, signal)
  }
  const expectedCode = normalizeLineEndings(action.text)
  const resolved = resolveInsertPosition(currentText, action.target)
  let position = resolved.position
  let remainingCode: string

  if (action.target.eof) {
    const currentCode = normalizeLineEndings(currentText)
    if (currentCode.endsWith(expectedCode)) {
      await facade.select(target, position, position)
      await facade.save(target)
      return 'done'
    }
    remainingCode = expectedCode.slice(
      suffixPrefixLength(currentCode, expectedCode),
    )
  } else if (
    action.target.at !== undefined &&
    typeof action.target.at !== 'number' &&
    effectivePlacement(action.target, 'insert') === 'before'
  ) {
    const codeBeforeAnchor = normalizeLineEndings(
      currentText.slice(0, resolved.offset),
    )
    const consumed = suffixPrefixLength(codeBeforeAnchor, expectedCode)
    if (consumed >= expectedCode.length) {
      await facade.select(target, position, position)
      await facade.save(target)
      return 'done'
    }
    remainingCode = expectedCode.slice(consumed)
  } else {
    const existing = await facade.matchAt(target, position, expectedCode)
    if (existing.matched >= expectedCode.length) {
      await facade.select(target, existing.position, existing.position)
      await facade.save(target)
      return 'done'
    }
    position = existing.position
    remainingCode = expectedCode.slice(existing.matched)
  }

  const eol = currentText.includes('\r\n') ? '\r\n' : '\n'
  const code = remainingCode.replace(/\r\n?|\n/g, eol)
  const chunks = splitIntoTypewriterChunks(
    code,
    speedMs,
    position.character === 0,
  )
  let currentEnd = position
  let undoGroupClosed = false

  return runChunkedEdit(
    {
      target,
      chunks,
      speedMs,
      prepare: () =>
        holdTarget(
          target,
          position,
          position,
          action.target_hold_ms,
          facade,
          signal,
        ),
      applyChunk: async (chunk, index, isLast) => {
        await facade.insert(target, currentEnd, chunk.text, {
          undoStopBefore: index === 0,
          undoStopAfter: isLast,
        })
        undoGroupClosed = isLast
        currentEnd = advancePosition(currentEnd, chunk.text)
      },
      restore: () =>
        rollbackInsertedText(
          target,
          facade,
          position,
          currentEnd,
          undoGroupClosed,
        ),
    },
    facade,
    signal,
  )
}

export async function applyDeleteText(
  action: DeleteTextAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const speedMs = action.speed_ms ?? 0
  validateTextEditSpeed('Deletion', speedMs)
  if (signal?.aborted) {
    return 'cancelled'
  }

  const target = await facade.existingTarget(action.path)
  const currentText = await facade.text(target)
  if (target.dirty) {
    return recoverDeleteText(action, target, currentText, facade, signal)
  }
  const direction = action.direction ?? 'backspace'
  const classification = classifyDeleteTarget(
    currentText,
    action.target,
    direction,
  )
  if (classification.kind === 'complete') {
    await facade.save(target)
    return 'done'
  }
  if (classification.kind === 'invalid') {
    throw classification.error
  }
  const { start, end } = classification.range

  const startPosition = positionAtOffset(currentText, start)
  const endPosition = positionAtOffset(currentText, end)
  const deletedText = currentText.slice(start, end)
  const chunks = splitIntoTypewriterChunks(deletedText, speedMs)
  if (direction === 'backspace') {
    chunks.reverse()
  }
  let removedOffset = end
  let currentEnd = endPosition

  return runChunkedEdit(
    {
      target,
      chunks,
      speedMs,
      prepare: async () => {
        if (action.target_hold_ms !== undefined) {
          const cursorTarget = 'cursor' in action.target
          const holdPosition =
            cursorTarget && direction === 'backspace'
              ? endPosition
              : startPosition
          return holdTarget(
            target,
            holdPosition,
            cursorTarget ? holdPosition : endPosition,
            action.target_hold_ms,
            facade,
            signal,
          )
        }
        if (speedMs > 0) {
          await facade.select(target, startPosition, endPosition)
          return !signal?.aborted
        }
        return true
      },
      applyChunk: async (chunk, index, isLast) => {
        if (direction === 'backspace') {
          const nextStartOffset = removedOffset - chunk.text.length
          const nextStart = positionAtOffset(currentText, nextStartOffset)
          await facade.replace(target, nextStart, currentEnd, '', {
            undoStopBefore: index === 0,
            undoStopAfter: isLast,
          })
          removedOffset = nextStartOffset
          currentEnd = nextStart
        } else {
          const chunkEnd = advancePosition(startPosition, chunk.text)
          await facade.replace(target, startPosition, chunkEnd, '', {
            undoStopBefore: index === 0,
            undoStopAfter: isLast,
          })
        }
      },
      restore: () => restoreDocumentText(target, facade, currentText),
    },
    facade,
    signal,
  )
}

function isCompletedReplacement(
  currentText: string,
  action: ReplaceTextAction,
): boolean {
  return (
    findTextMatches(currentText, action.text).length === 1 &&
    findTextMatches(currentText, action.target.expected_text).length === 0
  )
}

export async function applyReplaceText(
  action: ReplaceTextAction,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const speedMs = action.speed_ms ?? 0
  const editMode = action.edit_mode ?? 'overwrite_selection'
  if (editMode !== 'overwrite_selection' && editMode !== 'delete_then_type') {
    throw new ApplyError(
      'invalid_replacement_edit_mode',
      `Unsupported replacement edit mode: ${editMode}`,
    )
  }
  validateTextEditSpeed('Replacement', speedMs)
  if (speedMs === 0 && action.edit_mode !== undefined) {
    throw new ApplyError(
      'invalid_replacement_edit_mode',
      'Replacement edit mode requires a positive speed',
    )
  }
  if (signal?.aborted) {
    return 'cancelled'
  }

  const target = await facade.existingTarget(action.path)
  const currentText = await facade.text(target)
  if (target.dirty) {
    return recoverReplaceText(action, target, currentText, facade, signal)
  }
  let selection: TextMatch
  try {
    selection = resolveEditRangeRaw(currentText, action.target)
  } catch (error) {
    // A completed replacement can outlive its text anchors: the range is
    // gone, but the replacement is uniquely present and the old text is not
    // (ADR-0039). Ambiguous or malformed ranges still fail.
    if (
      error instanceof ApplyError &&
      error.code === 'code_match_missing' &&
      isCompletedReplacement(currentText, action)
    ) {
      if (signal?.aborted) {
        return 'cancelled'
      }
      await facade.save(target)
      return 'done'
    }
    throw error
  }
  if (signal?.aborted) {
    return 'cancelled'
  }

  const eol = currentText.includes('\r\n') ? '\r\n' : '\n'
  const replacement = action.text.replace(/\r\n?|\n/g, eol)
  const selectedText = currentText.slice(selection.start, selection.end)
  // A completed replacement replays as an idempotent no-op: the selected
  // target already holds the replacement code (ADR-0039). This must run
  // before the expected-text check because the replaced text is gone.
  if (
    normalizeLineEndings(selectedText) === normalizeLineEndings(action.text)
  ) {
    await facade.save(target)
    return 'done'
  }
  if (
    normalizeLineEndings(selectedText) !==
    normalizeLineEndings(action.target.expected_text)
  ) {
    throw new ApplyError(
      'stale_edit_range',
      'selected Edit Range does not match expected text',
    )
  }
  if (speedMs > 0 && editMode === 'delete_then_type') {
    return applyDeleteThenType(
      target,
      currentText,
      selection,
      replacement,
      action.target_hold_ms,
      speedMs,
      facade,
      signal,
    )
  }
  return applyOverwriteSelection(
    target,
    currentText,
    selection,
    replacement,
    action.target_hold_ms,
    speedMs,
    facade,
    signal,
  )
}

async function applyOverwriteSelection(
  target: PreparedTarget,
  currentText: string,
  selection: TextMatch,
  replacement: string,
  targetHoldMs: number | undefined,
  speedMs: number,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const selectedText = currentText.slice(selection.start, selection.end)
  const start = positionAtOffset(currentText, selection.start)
  const end = positionAtOffset(currentText, selection.end)
  const chunks = splitIntoTypewriterChunks(
    replacement,
    speedMs,
    startsAtLineStart(currentText, selection.start),
  )
  if (chunks.length === 0) {
    chunks.push({ text: '', units: 0 })
  }

  let currentEnd = start
  return runChunkedEdit(
    {
      target,
      chunks,
      speedMs,
      prepare: async () => {
        if (speedMs > 0 || targetHoldMs !== undefined) {
          return holdTarget(target, start, end, targetHoldMs, facade, signal)
        }
        return !signal?.aborted
      },
      applyChunk: async (chunk, index, isLast) => {
        if (index === 0) {
          await facade.replace(target, start, end, chunk.text, {
            undoStopBefore: true,
            undoStopAfter: isLast,
          })
        } else {
          await facade.insert(target, currentEnd, chunk.text, {
            undoStopBefore: false,
            undoStopAfter: isLast,
          })
        }
        currentEnd = advancePosition(
          index === 0 ? start : currentEnd,
          chunk.text,
        )
        if (speedMs > 0) {
          await facade.select(target, currentEnd, currentEnd)
        }
      },
      restore: () =>
        speedMs > 0
          ? rollbackTypewriterReplacement(
              target,
              facade,
              start,
              currentEnd,
              selectedText,
            )
          : restoreDocumentText(target, facade, currentText),
    },
    facade,
    signal,
  )
}

async function applyDeleteThenType(
  target: PreparedTarget,
  currentText: string,
  selection: TextMatch,
  replacement: string,
  targetHoldMs: number | undefined,
  speedMs: number,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  const selectedText = currentText.slice(selection.start, selection.end)
  const start = positionAtOffset(currentText, selection.start)
  let currentEnd = positionAtOffset(currentText, selection.end)
  const deletionChunks = splitIntoTypewriterChunks(selectedText, speedMs)
  deletionChunks.reverse()
  const insertionChunks = splitIntoTypewriterChunks(
    replacement,
    speedMs,
    startsAtLineStart(currentText, selection.start),
  )
  const chunks = [...deletionChunks, ...insertionChunks]
  let removedOffset = selection.end

  return runChunkedEdit(
    {
      target,
      chunks,
      speedMs,
      prepare: () =>
        holdTarget(target, start, currentEnd, targetHoldMs, facade, signal),
      applyChunk: async (chunk, index, isLast) => {
        if (index < deletionChunks.length) {
          const nextOffset = removedOffset - chunk.text.length
          const nextEnd = positionAtOffset(currentText, nextOffset)
          const lastDeletion = index === deletionChunks.length - 1
          await facade.replace(target, nextEnd, currentEnd, '', {
            undoStopBefore: index === 0,
            undoStopAfter: lastDeletion && insertionChunks.length === 0,
          })
          removedOffset = nextOffset
          currentEnd = nextEnd
        } else {
          const insertIndex = index - deletionChunks.length
          await facade.insert(target, currentEnd, chunk.text, {
            undoStopBefore: false,
            undoStopAfter: insertIndex === insertionChunks.length - 1,
          })
          currentEnd = advancePosition(currentEnd, chunk.text)
        }
        await facade.select(target, currentEnd, currentEnd)
      },
      delayAfter: (chunk, index, isLast) => {
        if (isLast || index === deletionChunks.length - 1) {
          return 0
        }
        return speedMs * chunk.units
      },
      restore: () =>
        rollbackTypewriterReplacement(
          target,
          facade,
          start,
          currentEnd,
          selectedText,
        ),
    },
    facade,
    signal,
  )
}

async function rollbackTypewriterReplacement(
  target: PreparedTarget,
  facade: EditorFacade,
  start: Position,
  end: Position,
  originalText: string,
): Promise<void> {
  await facade.replace(target, start, end, originalText, {
    undoStopBefore: false,
    undoStopAfter: true,
  })
  await facade.save(target)
}
function startsAtLineStart(text: string, offset: number): boolean {
  return offset === 0 || text[offset - 1] === '\n'
}

function validateTextEditSpeed(actionType: string, speedMs: number): void {
  if (!Number.isInteger(speedMs) || speedMs < 0) {
    throw new ApplyError(
      'invalid_typewriter_speed',
      `${actionType} speed must be a non-negative integer`,
    )
  }
}
async function holdTarget(
  target: PreparedTarget,
  start: Position,
  end: Position,
  holdMs: number | undefined,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<boolean> {
  await facade.select(target, start, end)
  if (signal?.aborted) {
    return false
  }
  if (holdMs !== undefined) {
    await facade.delay(holdMs, signal)
    if (signal?.aborted) {
      return false
    }
  }
  return true
}

async function restoreDocumentText(
  target: PreparedTarget,
  facade: EditorFacade,
  originalText: string,
): Promise<void> {
  const currentText = await facade.text(target)
  await facade.replace(
    target,
    { line: 0, character: 0 },
    positionAtOffset(currentText, currentText.length),
    originalText,
    { undoStopBefore: false, undoStopAfter: true },
  )
  await facade.save(target)
}
async function rollbackInsertedText(
  target: PreparedTarget,
  facade: EditorFacade,
  start: Position,
  end: Position,
  undoGroupClosed: boolean,
): Promise<void> {
  await facade.replace(target, start, end, '', {
    undoStopBefore: false,
    undoStopAfter: true,
  })
  if (!undoGroupClosed) {
    await facade.endUndoGroup(target)
  }
  await facade.save(target)
}
