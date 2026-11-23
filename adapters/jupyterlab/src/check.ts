import {
  parseCheckPayload as parseSharedCheckPayload,
  type CheckPayload,
} from '@cynapse/adapter-kit'
import { JupyterLabFacadeError } from './actions'
import type { AckPayload, Envelope } from './protocol'
import {
  applyTerminalAction,
  TerminalFacadeError,
  type TerminalFacade,
} from './terminal'
import { normalizeServerPath } from './terminalSessions'

export type { CheckPayload } from '@cynapse/adapter-kit'

/** Dirty Notebook paths come from the notebook facade, relative to the Jupyter root. */
export interface CheckNotebookFacade {
  dirtyNotebooks(): Promise<string[]>
  saveNotebook(path: string, signal: AbortSignal): Promise<void>
}

export interface CheckContext {
  signal: AbortSignal
}

export type CheckHandler = (
  envelope: Envelope,
  context: CheckContext,
) => Promise<AckPayload>

export function createJupyterLabCheckHandler(
  notebooks: CheckNotebookFacade,
  terminal: TerminalFacade,
): CheckHandler {
  return (envelope, { signal }) =>
    applyCheck(envelope.payload, notebooks, terminal, signal)
}

export async function applyCheck(
  payload: unknown,
  notebooks: CheckNotebookFacade,
  terminal: TerminalFacade,
  signal: AbortSignal,
): Promise<AckPayload> {
  const check = parseCheckPayload(payload)
  if (!check) {
    return {
      status: 'error',
      code: 'invalid_check',
      message: 'Invalid challenge check parameters',
    }
  }

  const cwd = resolveUnitCheckCwd(check.unit_path, check.cwd)
  if (cwd === undefined) {
    return {
      status: 'error',
      code: 'invalid_check',
      message: 'The challenge check directory escapes the unit workspace',
    }
  }

  try {
    const dirty = dirtyNotebooksUnder(
      check.unit_path,
      await notebooks.dirtyNotebooks(),
    )
    if (dirty.length > 0) {
      if (!check.save_dirty) {
        return {
          status: 'error',
          code: 'workspace_dirty',
          message: 'Save unsaved Notebooks before running the challenge check',
        }
      }
      for (const path of dirty) {
        await notebooks.saveNotebook(path, signal)
      }
      if (signal.aborted) {
        return { status: 'cancelled' }
      }
    }

    return await applyTerminalAction(
      {
        type: 'TerminalCommand',
        session_key: check.session_key,
        cwd,
        command: check.command,
        accepted_exit_codes: check.accepted_exit_codes,
        expected_output: check.expected_output,
        timeout_ms: check.timeout_ms,
      },
      terminal,
      signal,
    )
  } catch (error) {
    return {
      status: 'error',
      code: checkErrorCode(error),
      message: error instanceof Error ? error.message : String(error),
    }
  }
}

export function parseCheckPayload(payload: unknown): CheckPayload | undefined {
  return parseSharedCheckPayload(payload, {
    normalizeUnitPath: normalizeServerPath,
  })
}

export function resolveUnitCheckCwd(
  unitPath: string,
  cwd: string,
): string | undefined {
  const unit = normalizeServerPath(unitPath)
  if (unit === undefined || unit.length === 0) {
    return undefined
  }
  if (cwd.startsWith('/') || cwd.startsWith('\\') || /^[A-Za-z]:/.test(cwd)) {
    return undefined
  }
  const resolved = normalizeServerPath(
    cwd === '.' || cwd === './' ? unit : `${unit}/${cwd}`,
  )
  if (resolved === undefined) {
    return undefined
  }
  if (resolved === unit || resolved.startsWith(`${unit}/`)) {
    return resolved
  }
  return undefined
}

export function dirtyNotebooksUnder(
  unitPath: string,
  paths: readonly string[],
): string[] {
  const unit = normalizeServerPath(unitPath)
  if (unit === undefined) {
    return []
  }
  return paths.filter((path) => {
    const normalized = normalizeServerPath(path)
    return (
      normalized !== undefined &&
      (normalized === unit || normalized.startsWith(`${unit}/`))
    )
  })
}

function checkErrorCode(error: unknown): string {
  if (error instanceof TerminalFacadeError) {
    return error.code
  }
  if (error instanceof JupyterLabFacadeError) {
    return error.code
  }
  return 'check_failed'
}
