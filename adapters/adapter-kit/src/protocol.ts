/** Shared Protocol v2 message types used by every TypeScript Adapter. */
export const PROTOCOL_VERSION = 3

export type MessageType =
  | 'hello'
  | 'hello_ack'
  | 'action'
  | 'check'
  | 'ack'
  | 'cancel'
  | 'workspace_reset'

export interface Envelope {
  v: number
  id: string
  type: MessageType
  unit_path?: string
  payload: Record<string, unknown>
}

export interface HelloPayload {
  token: string
  role: string
}

export interface HelloAckPayload {
  adapter: string
}

export interface KernelErrorPayload {
  ename: string
  evalue: string
  traceback: string[]
}

export interface CellOutputSummary {
  output_type: 'stream' | 'execute_result' | 'display_data' | 'error'
  mime_types: string[]
  size: number
}

export interface AckPayload {
  status: 'done' | 'error' | 'cancelled'
  code?: string
  message?: string
  exit_code?: number
  execution_count?: number
  error?: KernelErrorPayload
  outputs?: CellOutputSummary[]
  outputs_truncated?: boolean
  output?: string
  output_truncated?: boolean
  completion_marker_found?: boolean
  timed_out?: boolean
}

export interface CheckPayload {
  type: 'COMMAND'
  unit_path: string
  session_key: string
  cwd: string
  command: string
  accepted_exit_codes: number[]
  expected_output: string[]
  timeout_ms: number
  save_dirty: boolean
}

export interface CancelPayload {
  target_id: string
  reason?: 'user_stop' | 'client_exit' | 'course_switch'
}

export interface WorkspaceResetPayload {
  paths: string[]
  exclude_paths?: string[]
  session_keys?: string[]
  close_tabs?: boolean
}

export interface TerminalEnsureAction {
  type: 'TerminalEnsure'
  session_key: string
  cwd: string
}

export interface TerminalCommandAction {
  type: 'TerminalCommand'
  session_key: string
  cwd: string
  command: string
  accepted_exit_codes: number[]
  expected_output: string[]
  timeout_ms: number
  completion_marker?: string
  new_terminal?: boolean
  /** Internal check-only capture: redirect output to a file the adapter reads. */
  capture_to_file?: boolean
}

export type TerminalAction = TerminalEnsureAction | TerminalCommandAction

/** Strict parser for the terminal Actions shared by both Adapters. */
export function parseTerminalAction(
  value: unknown,
): TerminalAction | undefined {
  if (!isRecord(value) || typeof value.type !== 'string') {
    return undefined
  }

  if (value.type === 'TerminalEnsure') {
    if (
      typeof value.session_key !== 'string' ||
      value.session_key.length === 0 ||
      typeof value.cwd !== 'string' ||
      value.cwd.length === 0 ||
      !hasOnlyKeys(value, ['type', 'session_key', 'cwd'])
    ) {
      return undefined
    }
    return {
      type: 'TerminalEnsure',
      session_key: value.session_key,
      cwd: value.cwd,
    }
  }

  if (value.type === 'TerminalCommand') {
    const acceptedExitCodes = value.accepted_exit_codes ?? [0]
    const expectedOutput = value.expected_output ?? []
    const timeoutMs = value.timeout_ms ?? 300_000
    if (
      typeof value.session_key !== 'string' ||
      value.session_key.length === 0 ||
      typeof value.cwd !== 'string' ||
      value.cwd.length === 0 ||
      typeof value.command !== 'string' ||
      value.command.length === 0 ||
      !isNonEmptyIntegerArray(acceptedExitCodes) ||
      !isNonEmptyStringArray(expectedOutput) ||
      typeof timeoutMs !== 'number' ||
      !Number.isInteger(timeoutMs) ||
      timeoutMs < 1 ||
      (value.completion_marker !== undefined &&
        (typeof value.completion_marker !== 'string' ||
          value.completion_marker.length === 0)) ||
      (value.new_terminal !== undefined &&
        typeof value.new_terminal !== 'boolean') ||
      !hasOnlyKeys(value, [
        'type',
        'session_key',
        'cwd',
        'command',
        'accepted_exit_codes',
        'expected_output',
        'timeout_ms',
        'completion_marker',
        'new_terminal',
      ])
    ) {
      return undefined
    }
    return {
      type: 'TerminalCommand',
      session_key: value.session_key,
      cwd: value.cwd,
      command: value.command,
      accepted_exit_codes: acceptedExitCodes,
      expected_output: expectedOutput,
      timeout_ms: timeoutMs,
      ...(value.completion_marker !== undefined
        ? { completion_marker: value.completion_marker }
        : {}),
      ...(value.new_terminal === true ? { new_terminal: true } : {}),
    }
  }

  return undefined
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function hasOnlyKeys(value: Record<string, unknown>, keys: string[]): boolean {
  return Object.keys(value).every((key) => keys.includes(key))
}

function isNonEmptyIntegerArray(value: unknown): value is number[] {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every((item) => typeof item === 'number' && Number.isInteger(item))
  )
}

function isNonEmptyStringArray(value: unknown): value is string[] {
  return (
    Array.isArray(value) &&
    value.every((item) => typeof item === 'string' && item.length > 0)
  )
}
