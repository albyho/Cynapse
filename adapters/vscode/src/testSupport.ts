import assert from 'node:assert/strict'
import { pathToFileURL } from 'node:url'
import * as path from 'node:path'
import { test } from 'node:test'
import {
  ApplyError,
  advancePosition,
  matchTextPrefix,
  resolveInsertPosition,
  type TextMatch,
  type DeleteTextAction,
  type EditorFacade,
  type EditRange,
  type InsertTextAction,
  type InsertOptions,
  type Position,
  type PreparedTarget,
  type ReplaceTextAction,
} from './apply'
import { applyAction } from './actions'
import { type ActionFacades } from './actions'
import type { FileSystemFacade } from './filesystem'

export class FakeFacade implements EditorFacade {
  readonly inserts: Array<{ position: Position; text: string }> = []
  readonly selections: Array<{ start: Position; end: Position }> = []
  readonly replacements: Array<{
    start: Position
    end: Position
    text: string
  }> = []
  private readonly openedPaths = new Set<string>()
  readonly delays: number[] = []
  undoStops = 0
  undoGroupsEnded = 0
  saved = false
  saves = 0
  alreadyTyped = false
  existingText = ''
  realDelay = false
  preparedPath: string | undefined
  readonly target: PreparedTarget = {
    uri: pathToFileURL(path.resolve('test-workspace/src/main.rs')).href,
    lineCount: 1,
    version: 1,
    savedText: '',
    dirty: false,
  }

  readonly previews: Array<{ path: string; view: string }> = []
  readonly reveals: Array<{ start: Position; end: Position }> = []
  clearReveals = 0
  closedSidePreviews = 0

  async closeSidePreviews(): Promise<void> {
    this.closedSidePreviews += 1
  }

  readonly closedFiles: string[] = []
  readonly closedPreviews: string[] = []

  async closeFile(filePath: string): Promise<void> {
    this.closedFiles.push(filePath)
  }

  async closePreview(filePath: string): Promise<void> {
    this.closedPreviews.push(filePath)
  }

  async preview(filePath: string, view: string): Promise<void> {
    this.previews.push({ path: filePath, view })
  }

  async reveal(
    _target: PreparedTarget,
    start: Position,
    end: Position,
  ): Promise<void> {
    this.reveals.push({ start, end })
  }

  async clearReveal(): Promise<void> {
    this.clearReveals += 1
  }

  async open(filePath: string, _placement?: 'first'): Promise<PreparedTarget> {
    const target = await this.prepareTarget(filePath)
    this.openedPaths.add(filePath)
    return target
  }

  async existingTarget(filePath: string): Promise<PreparedTarget> {
    if (!this.openedPaths.has(filePath)) {
      throw new ApplyError(
        'document_not_open',
        `${filePath} must be opened by OpenFile before it is edited`,
      )
    }
    this.preparedPath = filePath
    return this.target
  }

  async prepareTarget(filePath: string): Promise<PreparedTarget> {
    this.preparedPath = filePath
    return this.target
  }

  async insert(
    target: PreparedTarget,
    position: Position,
    text: string,
    options: InsertOptions,
  ): Promise<void> {
    this.inserts.push({ position, text })
    if (options.undoStopBefore || options.undoStopAfter) {
      this.undoStops += 1
    }
    const offset = offsetAt(this.existingText, position)
    this.existingText =
      this.existingText.slice(0, offset) +
      text +
      this.existingText.slice(offset)
    target.version += 1
  }

  async delay(ms: number, signal?: AbortSignal): Promise<void> {
    this.delays.push(ms)
    if (!this.realDelay || signal?.aborted) {
      return
    }
    await new Promise<void>((resolve) => {
      const finish = () => {
        clearTimeout(timer)
        signal?.removeEventListener('abort', finish)
        resolve()
      }
      const timer = setTimeout(finish, ms)
      signal?.addEventListener('abort', finish, { once: true })
    })
  }

  async save(): Promise<void> {
    this.saved = true
    this.saves += 1
  }

  async endUndoGroup(): Promise<void> {
    this.undoGroupsEnded += 1
    this.undoStops += 1
  }

  async matchAt(
    _target: PreparedTarget,
    position: Position,
    expected: string,
  ): Promise<TextMatch> {
    if (this.alreadyTyped) {
      return { matched: expected.length, position }
    }
    const startOffset = offsetAt(this.existingText, position)
    const match = matchTextPrefix(
      this.existingText.slice(startOffset),
      expected,
    )
    return {
      matched: match.matched,
      position: advancePosition(
        position,
        this.existingText.slice(0, match.consumed),
      ),
    }
  }

  async text(): Promise<string> {
    return this.existingText
  }

  async replace(
    _target: PreparedTarget,
    start: Position,
    end: Position,
    text: string,
  ): Promise<void> {
    this.replacements.push({ start, end, text })
    const startOffset = offsetAt(this.existingText, start)
    const endOffset = offsetAt(this.existingText, end)
    this.existingText =
      this.existingText.slice(0, startOffset) +
      text +
      this.existingText.slice(endOffset)
  }

  async select(
    _target: PreparedTarget,
    start: Position,
    end: Position,
  ): Promise<void> {
    this.selections.push({ start, end })
  }
}

export function offsetAt(text: string, position: Position): number {
  const lines = text.split('\n')
  let offset = position.character
  for (let line = 0; line < position.line; line += 1) {
    offset += lines[line].length + 1
  }
  return offset
}

export function replaceAction(
  overrides: Partial<ReplaceTextAction> = {},
): ReplaceTextAction {
  return {
    type: 'ReplaceText',
    path: 'variables/main.rs',
    target: textRange('let x = 5;', 'let x = 5;', 'let x = 5;\n'),
    text: 'let mut x = 5;\n',
    ...overrides,
  }
}

export function deleteAction(
  overrides: Partial<DeleteTextAction> = {},
): DeleteTextAction {
  return {
    type: 'DeleteText',
    path: 'src/main.rs',
    target: textRange('temporary'),
    ...overrides,
  }
}

export function textRange(
  startText: string,
  endText = startText,
  expectedText = startText,
  startPlacement?: 'before' | 'after',
  endPlacement?: 'before' | 'after',
): EditRange {
  return {
    start: {
      at: { text: startText },
      ...(startPlacement === undefined ? {} : { placement: startPlacement }),
    },
    end: {
      at: { text: endText },
      placement: endPlacement ?? 'after',
    },
    expected_text: expectedText,
  }
}

export function actionFacades(editor: EditorFacade): ActionFacades {
  return {
    editor,
    filesystem: {} as FileSystemFacade,
  }
}

export type OpenedEditAction =
  InsertTextAction | DeleteTextAction | ReplaceTextAction

export async function applyOpenedEdit(
  action: OpenedEditAction,
  facade: EditorFacade,
  signal?: AbortSignal,
) {
  const opened = await applyAction(
    { type: 'OpenFile', path: action.path },
    actionFacades(facade),
    signal,
  )
  if (opened === 'cancelled') {
    return opened
  }
  return applyAction(action, actionFacades(facade), signal)
}

export const applyOpenedReplace = applyOpenedEdit
export const applyOpenedDelete = applyOpenedEdit
export const applyOpenedInsert = applyOpenedEdit

export function insertAction(
  overrides: Partial<InsertTextAction> = {},
): InsertTextAction {
  return {
    type: 'InsertText',
    path: 'src/main.rs',
    target: { line: 1 },
    text: 'fn main() {}\n',
    ...overrides,
  }
}

export function crashedFacade(baseline: string, buffer: string): FakeFacade {
  const facade = new FakeFacade()
  facade.existingText = buffer
  facade.target.savedText = baseline
  facade.target.dirty = true
  return facade
}

export function assertSelfHealed(
  facade: FakeFacade,
  terminal: string,
  edits: number,
): void {
  assert.equal(facade.existingText, terminal)
  assert.deepEqual(facade.delays, [], 'recovery must not schedule delays')
  assert.equal(facade.saves, 1, 'recovery must save exactly once')
  assert.equal(facade.inserts.length + facade.replacements.length, edits)
}

export function assertDirtyRejected(facade: FakeFacade, buffer: string): void {
  assert.equal(facade.existingText, buffer, 'the buffer must stay untouched')
  assert.equal(facade.inserts.length, 0)
  assert.equal(facade.replacements.length, 0)
  assert.equal(facade.selections.length, 0)
  assert.equal(facade.saves, 0)
}

export function resolveInsertOffsetForTest(
  text: string,
  target: InsertTextAction['target'],
): number {
  return resolveInsertPosition(text, target).offset
}
