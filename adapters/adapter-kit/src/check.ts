import type { CheckPayload } from './protocol'

export interface CheckParseOptions {
  /** Normalizes and validates an Adapter-specific Unit path. */
  normalizeUnitPath?: (value: string) => string | undefined
}

export function parseCheckPayload(
  payload: unknown,
  options: CheckParseOptions = {},
): CheckPayload | undefined {
  if (!isRecord(payload) || payload.type !== 'COMMAND') {
    return undefined
  }
  if (
    typeof payload.unit_path !== 'string' ||
    payload.unit_path.length === 0 ||
    (options.normalizeUnitPath !== undefined &&
      options.normalizeUnitPath(payload.unit_path) === undefined) ||
    typeof payload.session_key !== 'string' ||
    payload.session_key.length === 0 ||
    typeof payload.command !== 'string' ||
    payload.command.length === 0
  ) {
    return undefined
  }
  if (payload.cwd !== undefined && typeof payload.cwd !== 'string') {
    return undefined
  }
  if (
    payload.accepted_exit_codes !== undefined &&
    (!Array.isArray(payload.accepted_exit_codes) ||
      payload.accepted_exit_codes.length === 0 ||
      !payload.accepted_exit_codes.every(
        (value) => typeof value === 'number' && Number.isInteger(value),
      ))
  ) {
    return undefined
  }
  if (
    payload.expected_output !== undefined &&
    (!Array.isArray(payload.expected_output) ||
      !payload.expected_output.every((value) => typeof value === 'string'))
  ) {
    return undefined
  }
  if (
    payload.timeout_ms !== undefined &&
    (typeof payload.timeout_ms !== 'number' ||
      !Number.isInteger(payload.timeout_ms) ||
      payload.timeout_ms <= 0)
  ) {
    return undefined
  }
  if (
    payload.save_dirty !== undefined &&
    typeof payload.save_dirty !== 'boolean'
  ) {
    return undefined
  }
  return {
    type: 'COMMAND',
    unit_path: payload.unit_path,
    session_key: payload.session_key,
    cwd: typeof payload.cwd === 'string' ? payload.cwd : '.',
    command: payload.command,
    accepted_exit_codes:
      payload.accepted_exit_codes === undefined
        ? [0]
        : (payload.accepted_exit_codes as number[]),
    expected_output:
      payload.expected_output === undefined
        ? []
        : (payload.expected_output as string[]),
    timeout_ms:
      payload.timeout_ms === undefined
        ? 300_000
        : (payload.timeout_ms as number),
    save_dirty: payload.save_dirty === true,
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
