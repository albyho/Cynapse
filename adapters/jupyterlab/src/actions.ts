import type {
  AckPayload,
  CellOutputSummary,
  CellScrollAlign,
  Envelope,
  JupyterLabAction,
  KernelErrorPayload,
} from './protocol'
import {
  type CellDeleteTarget,
  type CellEditRange,
  type CellInsertTarget,
} from './cellTextEdit'
import { parseJupyterLabAction, parseTerminalAction } from './protocol'
import { applyTerminalAction, type TerminalFacade } from './terminal'

/** Bounded text keeps large Cell output out of the Protocol. Rich MIME bytes
 * stay in JupyterLab; the ACK carries type/size summaries only. */
export const MAX_CELL_OUTPUT_CHARS = 32_000
export const MAX_CELL_OUTPUTS = 200
export const MAX_CELL_MIME_TYPES = 16
export const MAX_CELL_ERROR_CHARS = 8_000
export const MAX_CELL_TRACEBACK_LINES = 16
const MAX_MIME_TYPE_CHARS = 64
const TRUNCATION_MARKER = '[Cynapse: output truncated]\n'

export type JupyterLabErrorCode =
  | 'notebook_missing'
  | 'notebook_not_open'
  | 'notebook_close_incomplete'
  | 'cell_id_missing'
  | 'cell_id_duplicate'
  | 'cell_range_invalid'
  | 'cell_execution_failed'
  | 'execution_timeout'
  | 'kernel_dead'
  | 'kernel_shutdown_incomplete'
  | 'kernel_restarted'
  | 'kernel_restart_failed'
  | 'preview_missing'
  | 'preview_not_markdown'
  | 'cell_type_mismatch'
  | 'cell_not_generated'
  | 'cell_text_target_invalid'
  | 'save_conflict'

/** A typed failure raised by the JupyterLab facade. */
export class JupyterLabFacadeError extends Error {
  constructor(
    readonly code: JupyterLabErrorCode,
    message: string,
  ) {
    super(message)
    this.name = 'JupyterLabFacadeError'
  }
}

export interface CellExecution {
  execution_count?: number
  error?: KernelErrorPayload
  outputs: CellOutputSummary[]
  /** Bounded tail produced by the execution. */
  text: string
  text_truncated?: boolean
  outputs_truncated?: boolean
}

/**
 * The seam between the Adapter action handler and real JupyterLab APIs.
 * `runCell` resolves only after the matching `status: idle`, never after the
 * `runCells` request alone.
 */
export interface JupyterLabFacade {
  openNotebook(path: string, signal: AbortSignal): Promise<void>
  selectCell(
    path: string,
    cellId: string,
    scroll: boolean,
    align: CellScrollAlign,
    signal: AbortSignal,
  ): Promise<void>
  openPreview(path: string, signal: AbortSignal): Promise<void>
  closeDocument(path: string, signal: AbortSignal): Promise<void>
  insertText(
    path: string,
    cellId: string,
    target: CellInsertTarget,
    text: string,
    speedMs: number,
    targetHoldMs: number | undefined,
    signal: AbortSignal,
  ): Promise<void>
  deleteText(
    path: string,
    cellId: string,
    target: CellDeleteTarget,
    direction: 'backspace' | 'delete',
    speedMs: number,
    targetHoldMs: number | undefined,
    signal: AbortSignal,
  ): Promise<void>
  replaceText(
    path: string,
    cellId: string,
    target: CellEditRange,
    text: string,
    speedMs: number,
    targetHoldMs: number | undefined,
    signal: AbortSignal,
  ): Promise<void>
  insertCell(
    path: string,
    cellId: string,
    afterCellId: string | null,
    beforeCellId: string | null,
    cellType: 'code' | 'markdown',
    source: string,
    signal: AbortSignal,
  ): Promise<void>
  moveCell(
    path: string,
    cellId: string,
    direction: 'up' | 'down',
    count: number,
    signal: AbortSignal,
  ): Promise<void>
  duplicateCell(
    path: string,
    cellId: string,
    newCellId: string | null,
    signal: AbortSignal,
  ): Promise<void>
  deleteCell(path: string, cellId: string, signal: AbortSignal): Promise<void>
  saveNotebook(path: string, signal: AbortSignal): Promise<void>
  runCell(
    path: string,
    fromCellId: string | null,
    toCellId: string | null,
    timeoutMs: number,
    signal: AbortSignal,
  ): Promise<CellExecution>
  restartKernel(path: string, signal: AbortSignal): Promise<void>
  interruptKernel(path: string, signal: AbortSignal): Promise<void>
}

export interface ActionContext {
  signal: AbortSignal
}

export type ActionHandler = (
  envelope: Envelope,
  context: ActionContext,
) => Promise<AckPayload>

export function createJupyterLabActionHandler(
  facade: JupyterLabFacade,
  terminal?: TerminalFacade,
): ActionHandler {
  return async (envelope, { signal }) => {
    if (envelope.payload.type === 'Present') {
      return { status: 'done' }
    }

    const terminalAction = parseTerminalAction(envelope.payload)
    if (terminalAction) {
      if (!terminal) {
        return {
          status: 'error',
          code: 'unhandled_action',
          message: 'No JupyterLab terminal facade is installed',
        }
      }
      return applyTerminalAction(terminalAction, terminal, signal)
    }

    const action = parseJupyterLabAction(envelope.payload)
    if (!action) {
      return {
        status: 'error',
        code: 'invalid_action',
        message: `Invalid JupyterLab action payload: ${payloadType(envelope.payload)}`,
      }
    }
    return applyJupyterLabAction(action, facade, signal)
  }
}

export async function applyJupyterLabAction(
  action: JupyterLabAction,
  facade: JupyterLabFacade,
  signal: AbortSignal,
): Promise<AckPayload> {
  if (signal.aborted) {
    return { status: 'cancelled' }
  }

  // Cell edits share one contract: run the edit, skip the save if the learner
  // stopped mid-edit, then return an ACK that accounts for a stop during the
  // save. `run` may yield a value (CellRun's execution) for the ACK builder.
  const mutateAndSave = async <T>(
    path: string,
    run: () => Promise<T>,
    ack: (result: T) => AckPayload = () => ({ status: 'done' }),
  ): Promise<AckPayload> => {
    const result = await run()
    if (signal.aborted) {
      return { status: 'cancelled' }
    }
    await facade.saveNotebook(path, signal)
    return signal.aborted ? { status: 'cancelled' } : ack(result)
  }

  try {
    switch (action.type) {
      case 'JupyterLabOpenNotebook': {
        await facade.openNotebook(action.path, signal)
        return cancelledWhenAborted(signal)
      }
      case 'JupyterLabCellSelect': {
        await facade.selectCell(
          action.path,
          action.cell_id,
          action.scroll,
          action.align,
          signal,
        )
        return cancelledWhenAborted(signal)
      }
      case 'JupyterLabOpenPreview': {
        await facade.openPreview(action.path, signal)
        return cancelledWhenAborted(signal)
      }
      case 'JupyterLabCloseDocument': {
        await facade.closeDocument(action.path, signal)
        return cancelledWhenAborted(signal)
      }
      case 'JupyterLabCellInsertText': {
        return await mutateAndSave(action.path, () =>
          facade.insertText(
            action.path,
            action.cell_id,
            action.target,
            action.text,
            action.speed_ms,
            action.target_hold_ms,
            signal,
          ),
        )
      }
      case 'JupyterLabCellDeleteText': {
        return await mutateAndSave(action.path, () =>
          facade.deleteText(
            action.path,
            action.cell_id,
            action.target,
            action.direction,
            action.speed_ms,
            action.target_hold_ms,
            signal,
          ),
        )
      }
      case 'JupyterLabCellReplaceText': {
        return await mutateAndSave(action.path, () =>
          facade.replaceText(
            action.path,
            action.cell_id,
            action.target,
            action.text,
            action.speed_ms,
            action.target_hold_ms,
            signal,
          ),
        )
      }
      case 'JupyterLabCellInsert': {
        return await mutateAndSave(action.path, () =>
          facade.insertCell(
            action.path,
            action.cell_id,
            action.after_cell_id ?? null,
            action.before_cell_id ?? null,
            action.cell_type,
            action.source,
            signal,
          ),
        )
      }
      case 'JupyterLabCellMove': {
        return await mutateAndSave(action.path, () =>
          facade.moveCell(
            action.path,
            action.cell_id,
            action.direction,
            action.count,
            signal,
          ),
        )
      }
      case 'JupyterLabCellDuplicate': {
        return await mutateAndSave(action.path, () =>
          facade.duplicateCell(
            action.path,
            action.cell_id,
            action.new_cell_id ?? null,
            signal,
          ),
        )
      }
      case 'JupyterLabCellDelete': {
        return await mutateAndSave(action.path, () =>
          facade.deleteCell(action.path, action.cell_id, signal),
        )
      }
      case 'JupyterLabCellRun': {
        return await mutateAndSave(
          action.path,
          () =>
            facade.runCell(
              action.path,
              action.from_cell_id ?? null,
              action.to_cell_id ?? null,
              action.timeout_ms,
              signal,
            ),
          executionAck,
        )
      }
      case 'JupyterLabRestartKernel': {
        await facade.restartKernel(action.path, signal)
        return cancelledWhenAborted(signal)
      }
      case 'JupyterLabInterruptKernel': {
        await facade.interruptKernel(action.path, signal)
        return cancelledWhenAborted(signal)
      }
    }
  } catch (error) {
    if (
      error instanceof JupyterLabFacadeError &&
      (error.code === 'kernel_restarted' ||
        error.code === 'kernel_restart_failed')
    ) {
      return {
        status: error.code === 'kernel_restarted' ? 'cancelled' : 'error',
        code: error.code,
        message: error.message,
      }
    }
    if (signal.aborted) {
      return { status: 'cancelled' }
    }
    if (error instanceof JupyterLabFacadeError) {
      const ack: AckPayload = {
        status: 'error',
        code: error.code,
        message: error.message,
      }
      if (error.code === 'execution_timeout') {
        ack.timed_out = true
      }
      return ack
    }
    return {
      status: 'error',
      code: 'apply_failed',
      message: error instanceof Error ? error.message : String(error),
    }
  }
}

function cancelledWhenAborted(signal: AbortSignal): AckPayload {
  return signal.aborted ? { status: 'cancelled' } : { status: 'done' }
}

function executionAck(execution: CellExecution): AckPayload {
  const bounded = boundedOutputSummaries(execution.outputs)
  const ack: AckPayload = {
    status: 'done',
    outputs: bounded.outputs,
    output: boundedOutput(
      execution.text,
      execution.text_truncated || execution.text.length > MAX_CELL_OUTPUT_CHARS,
    ),
    output_truncated:
      execution.text_truncated === true ||
      execution.text.length > MAX_CELL_OUTPUT_CHARS,
  }
  if (execution.execution_count !== undefined) {
    ack.execution_count = execution.execution_count
  }
  if (execution.error) {
    ack.error = boundedKernelError(execution.error)
  }
  if (execution.outputs_truncated || bounded.truncated) {
    ack.outputs_truncated = true
  }
  return ack
}

export function boundedOutput(text: string, truncated?: boolean): string {
  const isTruncated = truncated ?? text.length > MAX_CELL_OUTPUT_CHARS
  if (!isTruncated) {
    return text
  }
  return (
    TRUNCATION_MARKER +
    (text.length > MAX_CELL_OUTPUT_CHARS
      ? text.slice(-MAX_CELL_OUTPUT_CHARS)
      : text)
  )
}

export function boundedKernelError(
  error: KernelErrorPayload,
): KernelErrorPayload {
  return {
    ename: error.ename.slice(0, MAX_CELL_ERROR_CHARS),
    evalue: error.evalue.slice(0, MAX_CELL_ERROR_CHARS),
    traceback: error.traceback
      .slice(-MAX_CELL_TRACEBACK_LINES)
      .map((line) => line.slice(0, MAX_CELL_ERROR_CHARS)),
  }
}

export function boundedOutputSummaries(outputs: CellOutputSummary[]): {
  outputs: CellOutputSummary[]
  truncated: boolean
} {
  return {
    outputs: outputs.slice(0, MAX_CELL_OUTPUTS).map((output) => ({
      ...output,
      mime_types: output.mime_types
        .slice(0, MAX_CELL_MIME_TYPES)
        .map((mimeType) => mimeType.slice(0, MAX_MIME_TYPE_CHARS)),
    })),
    truncated: outputs.length > MAX_CELL_OUTPUTS,
  }
}

function payloadType(payload: Record<string, unknown>): string {
  return typeof payload.type === 'string' ? payload.type : 'unknown'
}
