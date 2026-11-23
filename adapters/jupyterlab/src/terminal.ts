import {
  classifyTerminalResult,
  terminalDecisionAck,
} from '@cynapse/adapter-kit'
import type {
  AckPayload,
  TerminalAction,
  TerminalCommandAction,
} from './protocol'

export {
  MAX_TERMINAL_OUTPUT_BYTES,
  boundTerminalOutput,
  normalizeTerminalOutput,
} from '@cynapse/adapter-kit'

export interface TerminalExecutionResult {
  exitCode: number | undefined
  output: string
  outputTruncated?: boolean
  cancelled: boolean
  timedOut: boolean
}

/**
 * The seam between the Adapter and a Jupyter Server terminal. `execute`
 * recreates a missing session by key and working directory before running.
 */
export interface TerminalFacade {
  ensure(sessionKey: string, cwd: string, newTerminal?: boolean): Promise<void>
  completionMarkerExists(completionMarker: string): Promise<boolean>
  disposeUnderPaths(
    paths: string[],
    excludePaths?: string[],
    sessionKeys?: readonly string[],
  ): Promise<void>
  execute(
    action: TerminalCommandAction,
    signal?: AbortSignal,
  ): Promise<TerminalExecutionResult>
}

/** A typed terminal failure raised by a facade or the session manager. */
export class TerminalFacadeError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message)
    this.name = 'TerminalFacadeError'
  }
}

export async function applyTerminalAction(
  action: TerminalAction,
  facade: TerminalFacade,
  signal?: AbortSignal,
): Promise<AckPayload> {
  if (signal?.aborted) {
    return { status: 'cancelled' }
  }

  try {
    if (action.type === 'TerminalEnsure') {
      await facade.ensure(action.session_key, action.cwd)
      return { status: 'done' }
    }

    if (
      action.completion_marker &&
      (await facade.completionMarkerExists(action.completion_marker))
    ) {
      return { status: 'done', completion_marker_found: true }
    }

    const decision = classifyTerminalResult(
      await facade.execute(action, signal),
      {
        acceptedExitCodes: action.accepted_exit_codes,
        expectedOutput: action.expected_output,
      },
    )
    const missing = decision.missingExpectedOutput.join(' | ')
    return terminalDecisionAck(decision, {
      timedOut: `Command timed out after ${action.timeout_ms} ms`,
      noExitCode: 'Shell did not return an exit code',
      exitCode: `Command exited with code ${decision.exitCode}`,
      outputTruncated: `Terminal output was truncated before expected output could be verified: ${missing}`,
      outputMismatch: `Expected terminal output not found: ${missing}`,
    })
  } catch (error) {
    return {
      status: 'error',
      code:
        error instanceof TerminalFacadeError ? error.code : 'terminal_failed',
      message: error instanceof Error ? error.message : String(error),
    }
  }
}
