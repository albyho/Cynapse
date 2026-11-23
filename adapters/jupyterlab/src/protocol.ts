import type {
  CellAnchor,
  CellAnchorAt,
  CellDeleteTarget,
  CellEditRange,
  CellInsertTarget,
  CellTextAnchor,
} from './cellTextEdit'

export {
  PROTOCOL_VERSION,
  type AckPayload,
  type CancelPayload,
  type CellOutputSummary,
  type Envelope,
  type HelloAckPayload,
  type KernelErrorPayload,
  type MessageType,
  parseTerminalAction,
  type TerminalAction,
  type TerminalCommandAction,
  type TerminalEnsureAction,
  type WorkspaceResetPayload,
} from '@cynapse/adapter-kit'

/** Mirrors `WindowedList.ScrollToAlign` minus the `smart` heuristic. */
export const CELL_SCROLL_ALIGNS = [
  'auto',
  'start',
  'center',
  'top-center',
  'end',
] as const

export type CellScrollAlign = (typeof CELL_SCROLL_ALIGNS)[number]

export interface OpenNotebookAction {
  type: 'JupyterLabOpenNotebook'
  path: string
}

export interface SelectCellAction {
  type: 'JupyterLabCellSelect'
  path: string
  cell_id: string
  scroll: boolean
  align: CellScrollAlign
}

export interface PreviewAction {
  type: 'JupyterLabOpenPreview'
  path: string
}

export interface CloseDocumentAction {
  type: 'JupyterLabCloseDocument'
  path: string
}

export type CellReplaceEditMode = 'overwrite_selection' | 'delete_then_type'

export interface JupyterLabCellInsertTextAction {
  type: 'JupyterLabCellInsertText'
  path: string
  cell_id: string
  target: CellInsertTarget
  text: string
  speed_ms: number
  target_hold_ms?: number
}

export interface JupyterLabCellDeleteTextAction {
  type: 'JupyterLabCellDeleteText'
  path: string
  cell_id: string
  target: CellDeleteTarget
  direction: 'backspace' | 'delete'
  speed_ms: number
  target_hold_ms?: number
}

export interface JupyterLabCellReplaceTextAction {
  type: 'JupyterLabCellReplaceText'
  path: string
  cell_id: string
  target: CellEditRange
  text: string
  edit_mode?: CellReplaceEditMode
  speed_ms: number
  target_hold_ms?: number
}

export interface InsertCellAction {
  type: 'JupyterLabCellInsert'
  path: string
  cell_id: string
  after_cell_id?: string
  before_cell_id?: string
  cell_type: 'code' | 'markdown'
  source: string
}

export interface MoveCellAction {
  type: 'JupyterLabCellMove'
  path: string
  cell_id: string
  direction: 'up' | 'down'
  count: number
}

export interface DuplicateCellAction {
  type: 'JupyterLabCellDuplicate'
  path: string
  cell_id: string
  new_cell_id?: string
}

export interface DeleteCellAction {
  type: 'JupyterLabCellDelete'
  path: string
  cell_id: string
}

export interface RunCellAction {
  type: 'JupyterLabCellRun'
  path: string
  from_cell_id?: string
  to_cell_id?: string
  timeout_ms: number
}

export interface RestartKernelAction {
  type: 'JupyterLabRestartKernel'
  path: string
}

export interface InterruptKernelAction {
  type: 'JupyterLabInterruptKernel'
  path: string
}

export type JupyterLabAction =
  | OpenNotebookAction
  | SelectCellAction
  | PreviewAction
  | CloseDocumentAction
  | JupyterLabCellDeleteTextAction
  | JupyterLabCellInsertTextAction
  | JupyterLabCellReplaceTextAction
  | InsertCellAction
  | MoveCellAction
  | DuplicateCellAction
  | DeleteCellAction
  | RunCellAction
  | RestartKernelAction
  | InterruptKernelAction

export const DEFAULT_RUN_CELL_TIMEOUT_MS = 300_000
export const DEFAULT_MOVE_CELL_COUNT = 1

/** Strict parser for the Actions only the JupyterLab Adapter supports. */
export function parseJupyterLabAction(
  value: unknown,
): JupyterLabAction | undefined {
  if (!isRecord(value) || typeof value.type !== 'string') {
    return undefined
  }

  if (value.type === 'JupyterLabOpenNotebook') {
    if (!isNotebookPathAction(value)) {
      return undefined
    }
    return { type: 'JupyterLabOpenNotebook', path: value.path }
  }

  if (value.type === 'JupyterLabCellSelect') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      (value.scroll !== undefined && typeof value.scroll !== 'boolean') ||
      (value.align !== undefined && !isCellScrollAlign(value.align)) ||
      !hasOnlyKeys(value, ['type', 'path', 'cell_id', 'scroll', 'align'])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellSelect',
      path: value.path,
      cell_id: value.cell_id,
      scroll: value.scroll ?? true,
      align: value.align ?? 'auto',
    }
  }

  if (value.type === 'JupyterLabOpenPreview') {
    if (!isNotebookPathAction(value)) {
      return undefined
    }
    return { type: 'JupyterLabOpenPreview', path: value.path }
  }

  if (value.type === 'JupyterLabCloseDocument') {
    if (!isNotebookPathAction(value)) {
      return undefined
    }
    return { type: 'JupyterLabCloseDocument', path: value.path }
  }

  if (value.type === 'JupyterLabCellInsertText') {
    const speedMs = parseSpeedMs(value.speed_ms)
    const targetHoldMs = value.target_hold_ms
    const target = parseCellInsertTarget(value.target)
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      !target ||
      typeof value.text !== 'string' ||
      value.text.length === 0 ||
      speedMs === undefined ||
      (targetHoldMs !== undefined &&
        (typeof targetHoldMs !== 'number' ||
          !Number.isInteger(targetHoldMs) ||
          targetHoldMs < 1)) ||
      !hasOnlyKeys(value, [
        'type',
        'path',
        'cell_id',
        'target',
        'text',
        'speed_ms',
        'target_hold_ms',
      ])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellInsertText',
      path: value.path,
      cell_id: value.cell_id,
      target,
      text: value.text,
      speed_ms: speedMs,
      ...(targetHoldMs !== undefined ? { target_hold_ms: targetHoldMs } : {}),
    }
  }

  if (value.type === 'JupyterLabCellDeleteText') {
    const speedMs = parseSpeedMs(value.speed_ms)
    const targetHoldMs = value.target_hold_ms
    const target = parseCellDeleteTarget(value.target)
    const direction = value.direction ?? 'backspace'
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      !target ||
      (direction !== 'backspace' && direction !== 'delete') ||
      speedMs === undefined ||
      (targetHoldMs !== undefined &&
        (typeof targetHoldMs !== 'number' ||
          !Number.isInteger(targetHoldMs) ||
          targetHoldMs < 1)) ||
      !hasOnlyKeys(value, [
        'type',
        'path',
        'cell_id',
        'target',
        'direction',
        'speed_ms',
        'target_hold_ms',
      ])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellDeleteText',
      path: value.path,
      cell_id: value.cell_id,
      target,
      direction,
      speed_ms: speedMs,
      ...(targetHoldMs !== undefined ? { target_hold_ms: targetHoldMs } : {}),
    }
  }

  if (value.type === 'JupyterLabCellReplaceText') {
    const speedMs = parseSpeedMs(value.speed_ms)
    const targetHoldMs = value.target_hold_ms
    const target = parseCellEditRange(value.target)
    const editMode = value.edit_mode
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      !target ||
      typeof value.text !== 'string' ||
      value.text.length === 0 ||
      (editMode !== undefined &&
        editMode !== 'overwrite_selection' &&
        editMode !== 'delete_then_type') ||
      speedMs === undefined ||
      (targetHoldMs !== undefined &&
        (typeof targetHoldMs !== 'number' ||
          !Number.isInteger(targetHoldMs) ||
          targetHoldMs < 1)) ||
      !hasOnlyKeys(value, [
        'type',
        'path',
        'cell_id',
        'target',
        'text',
        'edit_mode',
        'speed_ms',
        'target_hold_ms',
      ])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellReplaceText',
      path: value.path,
      cell_id: value.cell_id,
      target,
      text: value.text,
      ...(editMode !== undefined ? { edit_mode: editMode } : {}),
      speed_ms: speedMs,
      ...(targetHoldMs !== undefined ? { target_hold_ms: targetHoldMs } : {}),
    }
  }

  if (value.type === 'JupyterLabCellInsert') {
    if (
      value.after_cell_id !== undefined &&
      value.before_cell_id !== undefined
    ) {
      return undefined
    }
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      (value.after_cell_id !== undefined &&
        (typeof value.after_cell_id !== 'string' ||
          value.after_cell_id.length === 0)) ||
      (value.before_cell_id !== undefined &&
        (typeof value.before_cell_id !== 'string' ||
          value.before_cell_id.length === 0)) ||
      (value.cell_type !== 'code' && value.cell_type !== 'markdown') ||
      typeof value.source !== 'string' ||
      !hasOnlyKeys(value, [
        'type',
        'path',
        'cell_id',
        'after_cell_id',
        'before_cell_id',
        'cell_type',
        'source',
      ])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellInsert',
      path: value.path,
      cell_id: value.cell_id,
      ...(value.after_cell_id !== undefined
        ? { after_cell_id: value.after_cell_id }
        : {}),
      ...(value.before_cell_id !== undefined
        ? { before_cell_id: value.before_cell_id }
        : {}),
      cell_type: value.cell_type,
      source: value.source,
    }
  }

  if (value.type === 'JupyterLabCellMove') {
    const count = value.count ?? DEFAULT_MOVE_CELL_COUNT
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      (value.direction !== 'up' && value.direction !== 'down') ||
      typeof count !== 'number' ||
      !Number.isInteger(count) ||
      count < 1 ||
      !hasOnlyKeys(value, ['type', 'path', 'cell_id', 'direction', 'count'])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellMove',
      path: value.path,
      cell_id: value.cell_id,
      direction: value.direction,
      count,
    }
  }

  if (value.type === 'JupyterLabCellDuplicate') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      (value.new_cell_id !== undefined &&
        (typeof value.new_cell_id !== 'string' ||
          value.new_cell_id.length === 0)) ||
      !hasOnlyKeys(value, ['type', 'path', 'cell_id', 'new_cell_id'])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellDuplicate',
      path: value.path,
      cell_id: value.cell_id,
      ...(value.new_cell_id !== undefined
        ? { new_cell_id: value.new_cell_id }
        : {}),
    }
  }

  if (value.type === 'JupyterLabCellDelete') {
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      typeof value.cell_id !== 'string' ||
      value.cell_id.length === 0 ||
      !hasOnlyKeys(value, ['type', 'path', 'cell_id'])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellDelete',
      path: value.path,
      cell_id: value.cell_id,
    }
  }

  if (value.type === 'JupyterLabCellRun') {
    const timeoutMs = value.timeout_ms ?? DEFAULT_RUN_CELL_TIMEOUT_MS
    const fromCellId = value.from_cell_id
    const toCellId = value.to_cell_id
    if (
      typeof value.path !== 'string' ||
      value.path.length === 0 ||
      (fromCellId !== undefined &&
        (typeof fromCellId !== 'string' || fromCellId.length === 0)) ||
      (toCellId !== undefined &&
        (typeof toCellId !== 'string' || toCellId.length === 0)) ||
      typeof timeoutMs !== 'number' ||
      !Number.isInteger(timeoutMs) ||
      timeoutMs < 1 ||
      !hasOnlyKeys(value, [
        'type',
        'path',
        'from_cell_id',
        'to_cell_id',
        'timeout_ms',
      ])
    ) {
      return undefined
    }
    return {
      type: 'JupyterLabCellRun',
      path: value.path,
      ...(fromCellId !== undefined ? { from_cell_id: fromCellId } : {}),
      ...(toCellId !== undefined ? { to_cell_id: toCellId } : {}),
      timeout_ms: timeoutMs,
    }
  }

  if (
    value.type === 'JupyterLabRestartKernel' ||
    value.type === 'JupyterLabInterruptKernel'
  ) {
    if (!isNotebookPathAction(value)) {
      return undefined
    }
    return { type: value.type, path: value.path }
  }

  return undefined
}

function parseCellInsertTarget(value: unknown): CellInsertTarget | undefined {
  return parseAnchor(value)
}

function parseAnchor(value: unknown): CellAnchor | undefined {
  if (!isRecord(value) || !hasOnlyKeys(value, ANCHOR_KEYS)) {
    return undefined
  }
  const line = value.line
  if (line !== undefined && (!Number.isInteger(line) || (line as number) < 1)) {
    return undefined
  }
  const at = value.at === undefined ? undefined : parseAnchorAt(value.at)
  if (value.at !== undefined && at === undefined) {
    return undefined
  }
  const placement = value.placement
  if (
    placement !== undefined &&
    placement !== 'before' &&
    placement !== 'after'
  ) {
    return undefined
  }
  const eof = value.eof
  if (eof !== undefined && typeof eof !== 'boolean') {
    return undefined
  }
  if (eof === true) {
    if (line !== undefined || at !== undefined || placement !== undefined) {
      return undefined
    }
    return { eof: true }
  }
  if (line === undefined && at === undefined) {
    return undefined
  }
  if (typeof at === 'number' && line === undefined) {
    return undefined
  }
  return {
    ...(line !== undefined ? { line: line as number } : {}),
    ...(at !== undefined ? { at } : {}),
    ...(placement !== undefined
      ? { placement: placement as CellAnchor['placement'] }
      : {}),
  }
}

function parseAnchorAt(value: unknown): CellAnchorAt | undefined {
  if (typeof value === 'number') {
    return Number.isInteger(value) && value >= 0 ? value : undefined
  }
  if (!isRecord(value) || !hasOnlyKeys(value, ['text', 'index'])) {
    return undefined
  }
  if (typeof value.text !== 'string' || value.text.length === 0) {
    return undefined
  }
  const index = value.index
  if (
    index !== undefined &&
    (!Number.isInteger(index) || (index as number) < 0)
  ) {
    return undefined
  }
  return {
    text: value.text,
    ...(index !== undefined ? { index: index as number } : {}),
  }
}

function parseCellEditRange(value: unknown): CellEditRange | undefined {
  if (!isRecord(value) || !hasOnlyKeys(value, EDIT_RANGE_KEYS)) {
    return undefined
  }
  const start = parseAnchor(value.start)
  const end = parseAnchor(value.end)
  if (
    !start ||
    !end ||
    start.eof ||
    typeof value.expected_text !== 'string' ||
    value.expected_text.length === 0
  ) {
    return undefined
  }
  if (isTextAt(start) && isTextAt(end)) {
    if (
      start.line === end.line &&
      start.at.text === end.at.text &&
      (start.at.index ?? 0) === (end.at.index ?? 0) &&
      (start.placement === 'after' || end.placement === 'before')
    ) {
      return undefined
    }
  }
  if (!isTextAt(start) && !isTextAt(end) && !end.eof) {
    const startLine = start.line
    const endLine = end.line
    if (startLine === undefined || endLine === undefined) {
      return undefined
    }
    if (
      startLine > endLine ||
      (startLine === endLine &&
        pointColumn(start, false) > pointColumn(end, true))
    ) {
      return undefined
    }
  }
  return { start, end, expected_text: value.expected_text }
}

const ANCHOR_KEYS = ['line', 'at', 'placement', 'eof']
const EDIT_RANGE_KEYS = ['start', 'end', 'expected_text']

function isTextAt(
  anchor: CellAnchor,
): anchor is CellAnchor & { at: CellTextAnchor } {
  return typeof anchor.at === 'object'
}

function pointColumn(anchor: CellAnchor, end: boolean): number {
  if (typeof anchor.at === 'number') {
    return anchor.at
  }
  const placement = anchor.placement ?? (end ? 'after' : 'before')
  return placement === 'after' ? Number.MAX_SAFE_INTEGER : 0
}

function parseCellDeleteTarget(value: unknown): CellDeleteTarget | undefined {
  const range = parseCellEditRange(value)
  if (range) {
    return range
  }
  if (!isRecord(value) || !isRecord(value.cursor)) {
    return undefined
  }
  const cursor = value.cursor
  if (
    typeof value.expected_text !== 'string' ||
    value.expected_text.length === 0 ||
    !Number.isInteger(cursor.line) ||
    (cursor.line as number) < 1 ||
    !Number.isInteger(cursor.column) ||
    (cursor.column as number) < 0 ||
    !Number.isInteger(cursor.count) ||
    (cursor.count as number) < 1 ||
    !hasOnlyKeys(value, ['cursor', 'expected_text']) ||
    !hasOnlyKeys(cursor, ['line', 'column', 'count'])
  ) {
    return undefined
  }
  return {
    cursor: {
      line: cursor.line as number,
      column: cursor.column as number,
      count: cursor.count as number,
    },
    expected_text: value.expected_text,
  }
}

function isCellScrollAlign(value: unknown): value is CellScrollAlign {
  return (
    typeof value === 'string' &&
    (CELL_SCROLL_ALIGNS as readonly string[]).includes(value)
  )
}

function isNotebookPathAction(
  value: Record<string, unknown>,
): value is { type: string; path: string } {
  return (
    typeof value.path === 'string' &&
    value.path.length > 0 &&
    hasOnlyKeys(value, ['type', 'path'])
  )
}

function parseSpeedMs(value: unknown): number | undefined {
  if (value === undefined || value === null) {
    return 0
  }
  return typeof value === 'number' && Number.isInteger(value) && value >= 0
    ? value
    : undefined
}

function hasOnlyKeys(
  value: Record<string, unknown>,
  allowed: string[],
): boolean {
  return Object.keys(value).every((key) => allowed.includes(key))
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
