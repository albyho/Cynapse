import {
  applyCloseFile,
  applyClosePreview,
  applyDeleteText,
  applyInsertText,
  applyOpenFile,
  applyOpenPreview,
  applyOpenFileWithPreview,
  applyReplaceText,
  applyRevealText,
  applyClearReveal,
  type ClearRevealAction,
  type CloseFileAction,
  type ClosePreviewAction,
  type EditorFacade,
  type Anchor,
  type DeleteTextAction,
  type EditRange,
  type InsertTextAction,
  type OpenFileAction,
  type OpenPreviewAction,
  type OpenFileWithPreviewAction,
  type RevealTextAction,
  type RevealPosition,
  type ReplaceTextAction,
} from './apply'
import {
  applyFileSystemAction,
  type FileSystemAction,
  type FileSystemFacade,
} from './filesystem'
import {
  parseTerminalAction,
  type TerminalCommandAction,
  type TerminalEnsureAction,
} from '@cynapse/adapter-kit'

export type TerminalAction = TerminalEnsureAction | TerminalCommandAction
export type NonTerminalAction =
  | PresentAction
  | OpenPreviewAction
  | OpenFileAction
  | OpenFileWithPreviewAction
  | CloseFileAction
  | ClosePreviewAction
  | RevealTextAction
  | ClearRevealAction
  | InsertTextAction
  | DeleteTextAction
  | ReplaceTextAction
  | FileSystemAction

export type CourseAction = NonTerminalAction | TerminalAction

export interface PresentAction {
  type: 'Present'
}

export interface ActionFacades {
  editor: EditorFacade
  filesystem: FileSystemFacade
}

export function parseCourseAction(value: unknown): CourseAction | undefined {
  if (!isRecord(value) || typeof value.type !== 'string') {
    return undefined
  }

  if (value.type === 'TerminalEnsure' || value.type === 'TerminalCommand') {
    return parseTerminalAction(value)
  }

  if (value.type === 'Present') {
    if (Object.keys(value).some((key) => key !== 'type')) {
      return undefined
    }
    return value as unknown as PresentAction
  }

  if (value.type === 'OpenPreview') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      (value.view !== undefined &&
        value.view !== 'side' &&
        value.view !== 'current') ||
      Object.keys(value).some((key) => !['type', 'path', 'view'].includes(key))
    ) {
      return undefined
    }
    return {
      type: 'OpenPreview',
      path: value.path,
      view: value.view === 'current' ? 'current' : 'side',
    }
  }

  if (value.type === 'OpenFile') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      Object.keys(value).some((key) => !['type', 'path'].includes(key))
    ) {
      return undefined
    }
    return value as unknown as OpenFileAction
  }

  if (value.type === 'OpenFileWithPreview') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      Object.keys(value).some((key) => !['type', 'path'].includes(key))
    ) {
      return undefined
    }
    return value as unknown as OpenFileWithPreviewAction
  }

  if (value.type === 'CloseFile') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      Object.keys(value).some((key) => !['type', 'path'].includes(key))
    ) {
      return undefined
    }
    return value as unknown as CloseFileAction
  }

  if (value.type === 'ClosePreview') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      Object.keys(value).some((key) => !['type', 'path'].includes(key))
    ) {
      return undefined
    }
    return value as unknown as ClosePreviewAction
  }

  if (value.type === 'RevealText') {
    const start = parseRevealPosition(value.start)
    const end =
      value.end === undefined ? undefined : parseRevealPosition(value.end)
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      !start ||
      (value.end !== undefined && !end) ||
      Object.keys(value).some(
        (key) => !['type', 'path', 'start', 'end'].includes(key),
      ) ||
      (end !== undefined && compareRevealPositions(end, start) < 0)
    ) {
      return undefined
    }
    return { type: 'RevealText', path: value.path, start, end }
  }

  if (value.type === 'ClearReveal') {
    if (Object.keys(value).some((key) => key !== 'type')) {
      return undefined
    }
    return { type: 'ClearReveal' }
  }

  if (value.type === 'InsertText') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      !isAnchor(value.target) ||
      typeof value.text !== 'string' ||
      value.text.length === 0 ||
      !isOptionalNonNegativeInteger(value.speed_ms) ||
      !isOptionalPositiveInteger(value.target_hold_ms) ||
      Object.keys(value).some(
        (key) =>
          ![
            'type',
            'path',
            'target',
            'text',
            'speed_ms',
            'target_hold_ms',
          ].includes(key),
      )
    ) {
      return undefined
    }
    return value as unknown as InsertTextAction
  }

  if (value.type === 'DeleteText') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      (!isEditRange(value.target) && !isCursorDeletionTarget(value.target)) ||
      !isOptionalDeleteDirection(value.direction) ||
      (isCursorDeletionTarget(value.target) && value.direction === undefined) ||
      !isOptionalNonNegativeInteger(value.speed_ms) ||
      !isOptionalPositiveInteger(value.target_hold_ms) ||
      Object.keys(value).some(
        (key) =>
          ![
            'type',
            'path',
            'target',
            'direction',
            'speed_ms',
            'target_hold_ms',
          ].includes(key),
      )
    ) {
      return undefined
    }
    return {
      ...value,
      type: 'DeleteText',
    } as unknown as DeleteTextAction
  }

  if (value.type === 'ReplaceText') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      !isEditRange(value.target) ||
      typeof value.text !== 'string' ||
      value.text.length === 0 ||
      !isReplaceEditMode(value.edit_mode, value.speed_ms) ||
      !isOptionalNonNegativeInteger(value.speed_ms) ||
      !isOptionalPositiveInteger(value.target_hold_ms) ||
      Object.keys(value).some(
        (key) =>
          ![
            'type',
            'path',
            'target',
            'text',
            'edit_mode',
            'speed_ms',
            'target_hold_ms',
          ].includes(key),
      )
    ) {
      return undefined
    }
    return value as unknown as ReplaceTextAction
  }

  if (typeof value.path !== 'string' || value.path.length === 0) {
    return undefined
  }

  if (value.type === 'CreateDirectory' || value.type === 'CreateFile') {
    if (
      !isOptionalBoolean(value, 'create_parents') ||
      'recursive' in value ||
      'if_missing' in value
    ) {
      return undefined
    }
    return value as unknown as FileSystemAction
  }
  if (value.type === 'DeleteFile') {
    if (
      'create_parents' in value ||
      'recursive' in value ||
      !isMissingPolicy(value.if_missing)
    ) {
      return undefined
    }
    return value as unknown as FileSystemAction
  }
  if (value.type === 'DeleteDirectory') {
    if (
      'create_parents' in value ||
      !isOptionalBoolean(value, 'recursive') ||
      !isMissingPolicy(value.if_missing)
    ) {
      return undefined
    }
    return value as unknown as FileSystemAction
  }

  return undefined
}

export function writeActionPath(action: CourseAction): string | undefined {
  switch (action.type) {
    case 'InsertText':
    case 'DeleteText':
    case 'ReplaceText':
    case 'CreateDirectory':
    case 'CreateFile':
    case 'DeleteFile':
    case 'DeleteDirectory':
      return action.path
    default:
      return undefined
  }
}

export async function applyAction(
  action: NonTerminalAction,
  facades: ActionFacades,
  signal?: AbortSignal,
): Promise<'done' | 'cancelled'> {
  if (action.type === 'Present') {
    return 'done'
  }
  if (action.type === 'OpenFile') {
    return applyOpenFile(action, facades.editor, signal)
  }
  if (action.type === 'OpenPreview') {
    return applyOpenPreview(action, facades.editor, signal)
  }
  if (action.type === 'OpenFileWithPreview') {
    return applyOpenFileWithPreview(action, facades.editor, signal)
  }
  if (action.type === 'CloseFile') {
    return applyCloseFile(action, facades.editor, signal)
  }
  if (action.type === 'ClosePreview') {
    return applyClosePreview(action, facades.editor, signal)
  }
  if (action.type === 'RevealText') {
    return applyRevealText(action, facades.editor, signal)
  }
  if (action.type === 'ClearReveal') {
    return applyClearReveal(action, facades.editor, signal)
  }
  if (action.type === 'InsertText') {
    return applyInsertText(action, facades.editor, signal)
  }
  if (action.type === 'DeleteText') {
    return applyDeleteText(action, facades.editor, signal)
  }
  if (action.type === 'ReplaceText') {
    return applyReplaceText(action, facades.editor, signal)
  }
  return applyFileSystemAction(action, facades.filesystem, signal)
}

function parseRevealPosition(value: unknown): RevealPosition | undefined {
  if (!isRecord(value)) {
    return undefined
  }
  if (Object.keys(value).some((key) => key !== 'line' && key !== 'column')) {
    return undefined
  }
  if (!Number.isInteger(value.line) || (value.line as number) < 1) {
    return undefined
  }
  const column = value.column
  if (column !== undefined) {
    if (!Number.isInteger(column) || (column as number) < 0) {
      return undefined
    }
  }
  return {
    line: value.line as number,
    column: column as number | undefined,
  }
}

function compareRevealPositions(a: RevealPosition, b: RevealPosition): number {
  if (a.line !== b.line) {
    return a.line - b.line
  }
  return (a.column ?? 0) - (b.column ?? 0)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isOptionalBoolean(
  value: Record<string, unknown>,
  key: string,
): boolean {
  return !(key in value) || typeof value[key] === 'boolean'
}

function isMissingPolicy(value: unknown): boolean {
  return value === undefined || value === 'error' || value === 'ignore'
}

function isEditRange(value: unknown): value is EditRange {
  if (
    !isRecord(value) ||
    Array.isArray(value) ||
    Object.keys(value).some(
      (key) => !['start', 'end', 'expected_text'].includes(key),
    ) ||
    !isAnchor(value.start) ||
    !isAnchor(value.end) ||
    typeof value.expected_text !== 'string' ||
    value.expected_text.length === 0
  ) {
    return false
  }

  const start = value.start
  const end = value.end
  if (start.eof) {
    return false
  }
  if (isTextAt(start.at) && isTextAt(end.at)) {
    if (
      start.line === end.line &&
      start.at.text === end.at.text &&
      (start.at.index ?? 0) === (end.at.index ?? 0) &&
      (start.placement === 'after' || end.placement === 'before')
    ) {
      return false
    }
  }
  if (!isTextAt(start.at) && !start.eof && !isTextAt(end.at) && !end.eof) {
    if (start.line === undefined || end.line === undefined) {
      return false
    }
    if (
      start.line > end.line ||
      (start.line === end.line &&
        pointColumn(start, false) > pointColumn(end, true))
    ) {
      return false
    }
  }
  return true
}

function isTextAt(
  value: Anchor['at'],
): value is { text: string; index?: number } {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Static ordering proxy: a line-level `after` runs to the end of its line. */
function pointColumn(anchor: Anchor, end: boolean): number {
  if (typeof anchor.at === 'number') {
    return anchor.at
  }
  const placement = anchor.placement ?? (end ? 'after' : 'before')
  return placement === 'before' ? 0 : Number.MAX_SAFE_INTEGER
}

function isAnchor(value: unknown): value is Anchor {
  if (!isRecord(value) || Array.isArray(value)) {
    return false
  }
  if (
    Object.keys(value).some(
      (key) => !['line', 'at', 'placement', 'eof'].includes(key),
    )
  ) {
    return false
  }
  if (
    value.line !== undefined &&
    (!Number.isInteger(value.line) || (value.line as number) < 1)
  ) {
    return false
  }
  if (
    value.placement !== undefined &&
    value.placement !== 'before' &&
    value.placement !== 'after'
  ) {
    return false
  }
  if (value.eof !== undefined && typeof value.eof !== 'boolean') {
    return false
  }
  if (value.at !== undefined && !isAnchorAt(value.at)) {
    return false
  }
  if (value.eof === true) {
    return (
      value.line === undefined &&
      value.at === undefined &&
      value.placement === undefined
    )
  }
  if (value.at === undefined && value.line === undefined) {
    return false
  }
  return typeof value.at !== 'number' || value.line !== undefined
}

function isAnchorAt(value: unknown): value is Anchor['at'] {
  if (typeof value === 'number') {
    return Number.isInteger(value) && value >= 0
  }
  if (!isRecord(value) || Array.isArray(value)) {
    return false
  }
  if (
    Object.keys(value).some((key) => key !== 'text' && key !== 'index') ||
    typeof value.text !== 'string' ||
    value.text.length === 0
  ) {
    return false
  }
  return (
    value.index === undefined ||
    (Number.isInteger(value.index) && (value.index as number) >= 0)
  )
}

function isCursorDeletionTarget(value: unknown): boolean {
  if (
    !isRecord(value) ||
    Object.keys(value).some(
      (key) => !['cursor', 'expected_text'].includes(key),
    ) ||
    !isRecord(value.cursor) ||
    typeof value.expected_text !== 'string' ||
    value.expected_text.length === 0
  ) {
    return false
  }
  const cursor = value.cursor
  return (
    Object.keys(cursor).every((key) =>
      ['line', 'column', 'count'].includes(key),
    ) &&
    Number.isInteger(cursor.line) &&
    (cursor.line as number) >= 1 &&
    Number.isInteger(cursor.column) &&
    (cursor.column as number) >= 0 &&
    Number.isInteger(cursor.count) &&
    (cursor.count as number) >= 1
  )
}

function isOptionalDeleteDirection(value: unknown): boolean {
  return value === undefined || value === 'backspace' || value === 'delete'
}

function isReplaceEditMode(value: unknown, speedMs: unknown): boolean {
  if (value === undefined) {
    return true
  }
  return (
    typeof speedMs === 'number' &&
    Number.isInteger(speedMs) &&
    speedMs >= 1 &&
    (value === 'overwrite_selection' || value === 'delete_then_type')
  )
}

function isOptionalNonNegativeInteger(value: unknown): boolean {
  return (
    value === undefined ||
    value === null ||
    (typeof value === 'number' && Number.isInteger(value) && value >= 0)
  )
}

function isOptionalPositiveInteger(value: unknown): boolean {
  return (
    value === undefined ||
    (typeof value === 'number' && Number.isInteger(value) && value >= 1)
  )
}
