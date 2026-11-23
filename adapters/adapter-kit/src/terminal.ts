import type { AckPayload } from './protocol'

export const MAX_TERMINAL_OUTPUT_BYTES = 128 * 1024

export const TERMINAL_TRUNCATION_MARKER = '[Cynapse: output truncated]\n'

export function normalizeTerminalOutput(output: string): string {
  return stripAnsi(output).replace(/\r\n?/g, '\n')
}

export interface TerminalResultInput {
  exitCode: number | undefined
  output: string
  outputTruncated?: boolean
  cancelled: boolean
  timedOut: boolean
}

export interface TerminalExpectations {
  acceptedExitCodes: readonly number[]
  expectedOutput: readonly string[]
}

export type TerminalResultKind =
  | 'cancelled'
  | 'timed_out'
  | 'exit_code_rejected'
  | 'output_truncated'
  | 'output_mismatch'
  | 'done'

export interface TerminalResultDecision {
  kind: TerminalResultKind
  exitCode: number | undefined
  output: string
  outputTruncated: boolean
  missingExpectedOutput: string[]
}

export function classifyTerminalResult(
  result: TerminalResultInput,
  expectations: TerminalExpectations,
): TerminalResultDecision {
  const bounded = boundTerminalOutput(
    result.output,
    result.outputTruncated ?? false,
  )
  const base = {
    exitCode: result.exitCode,
    output: bounded.output,
    outputTruncated: bounded.truncated,
    missingExpectedOutput: [],
  }

  if (result.cancelled) {
    return { ...base, kind: 'cancelled' }
  }
  if (result.timedOut) {
    return { ...base, kind: 'timed_out' }
  }
  if (
    result.exitCode === undefined ||
    !expectations.acceptedExitCodes.includes(result.exitCode)
  ) {
    return { ...base, kind: 'exit_code_rejected' }
  }

  const normalizedOutput = normalizeTerminalOutput(result.output)
  const missingExpectedOutput = expectations.expectedOutput.filter(
    (expected) => !normalizedOutput.includes(normalizeTerminalOutput(expected)),
  )
  if (missingExpectedOutput.length > 0) {
    return {
      ...base,
      kind:
        result.outputTruncated === true
          ? 'output_truncated'
          : 'output_mismatch',
      missingExpectedOutput,
    }
  }
  return { ...base, kind: 'done' }
}

export function boundTerminalOutput(
  raw: string,
  alreadyTruncated = false,
): {
  output: string
  truncated: boolean
} {
  const output = normalizeTerminalOutput(raw)
  const bytes = new TextEncoder().encode(output)
  if (!alreadyTruncated && bytes.length <= MAX_TERMINAL_OUTPUT_BYTES) {
    return { output, truncated: false }
  }

  const markerBytes = new TextEncoder().encode(TERMINAL_TRUNCATION_MARKER)
  const tailBytes = MAX_TERMINAL_OUTPUT_BYTES - markerBytes.length
  return {
    output:
      TERMINAL_TRUNCATION_MARKER +
      decodeUtf8Tail(bytes.subarray(bytes.length - tailBytes)),
    truncated: true,
  }
}

function decodeUtf8Tail(bytes: Uint8Array): string {
  let start = 0
  // Do not start decoding in the middle of a UTF-8 sequence.
  while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) {
    start += 1
  }
  return new TextDecoder().decode(bytes.subarray(start))
}

function stripAnsi(output: string): string {
  return output.replace(
    // CSI, OSC and single-character ESC sequences emitted by common terminals.
    // eslint-disable-next-line no-control-regex
    /\u001b(?:\][^\u0007]*(?:\u0007|\u001b\\)|\[[0-?]*[ -/]*[@-~]|[@-_])/g,
    '',
  )
}

/** User-facing text for a terminal decision, already formatted by the Adapter. */
export interface TerminalAckMessages {
  timedOut: string
  noExitCode: string
  exitCode: string
  outputTruncated: string
  outputMismatch: string
}

/**
 * Map a classified terminal result to its ACK. The Adapters own execution,
 * error handling, and message text (VS Code localizes it, others interpolate
 * it); only this decision-to-ACK structure is shared.
 */
export function terminalDecisionAck(
  decision: TerminalResultDecision,
  messages: TerminalAckMessages,
): AckPayload {
  const output = {
    ...(decision.exitCode !== undefined
      ? { exit_code: decision.exitCode }
      : {}),
    output: decision.output,
    output_truncated: decision.outputTruncated,
  }
  switch (decision.kind) {
    case 'cancelled':
      return { status: 'cancelled', ...output }
    case 'timed_out':
      return {
        status: 'cancelled',
        code: 'terminal_timeout',
        timed_out: true,
        message: messages.timedOut,
        ...output,
      }
    case 'exit_code_rejected':
      return {
        status: 'error',
        code: 'terminal_exit_code_rejected',
        message:
          decision.exitCode === undefined
            ? messages.noExitCode
            : messages.exitCode,
        ...output,
      }
    case 'output_truncated':
      return {
        status: 'error',
        code: 'terminal_output_truncated',
        message: messages.outputTruncated,
        ...output,
        output_truncated: true,
      }
    case 'output_mismatch':
      return {
        status: 'error',
        code: 'terminal_output_mismatch',
        message: messages.outputMismatch,
        ...output,
      }
    case 'done':
      return { status: 'done', ...output }
  }
}
