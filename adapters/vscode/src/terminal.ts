import { open } from 'node:fs/promises'

import { ApplyError } from './apply'
import { t } from './i18n'
import {
  sameTerminalDirectory,
  terminalDirectoryCommand,
} from './terminalSessions'
import {
  MAX_TERMINAL_OUTPUT_BYTES,
  classifyTerminalResult,
  terminalDecisionAck,
  type AckPayload,
  type TerminalCommandAction,
  type TerminalEnsureAction,
} from '@cynapse/adapter-kit'

export { MAX_TERMINAL_OUTPUT_BYTES } from '@cynapse/adapter-kit'

export interface TerminalExecutionResult {
  exitCode: number | undefined
  output: string
  outputTruncated?: boolean
  cancelled: boolean
  timedOut: boolean
}

export interface InterruptibleExecution {
  completion: Promise<{
    exitCode: number | undefined
    output: string
    outputTruncated?: boolean
  }>
  partialOutput(): string
  partialOutputTruncated?: () => boolean
  interrupt(): void
  timeoutMs: number
}

export interface TerminalFacade {
  ensure(
    sessionKey: string,
    cwd: string,
    preserveFocus: boolean,
    newTerminal?: boolean,
  ): Promise<void>
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

export type TerminalAction = TerminalEnsureAction | TerminalCommandAction

export interface DirectoryTerminalExecution {
  currentDirectory?: string
  targetDirectory: string
  shell?: string
  command: string
  timeoutMs: number
  execute(command: string, timeoutMs: number): Promise<TerminalExecutionResult>
}

export interface OutputCollector {
  partial(): string
  truncated(): boolean
}

export async function executeTerminalCommandInDirectory(
  execution: DirectoryTerminalExecution,
): Promise<TerminalExecutionResult> {
  if (
    !sameTerminalDirectory(
      execution.currentDirectory,
      execution.targetDirectory,
    )
  ) {
    const directoryChange = await execution.execute(
      terminalDirectoryCommand(execution.shell, execution.targetDirectory),
      Math.min(execution.timeoutMs, 5_000),
    )
    if (directoryChange.cancelled || directoryChange.timedOut) {
      return directoryChange
    }
    if (directoryChange.exitCode !== 0) {
      const detail = directoryChange.output.trim()
      throw new ApplyError(
        'terminal_cwd_change_failed',
        t(
          'Could not switch the Cynapse terminal to {0}{1}',
          execution.targetDirectory,
          detail ? `: ${detail}` : '',
        ),
      )
    }
  }
  return execution.execute(execution.command, execution.timeoutMs)
}

export async function awaitInterruptibleExecution(
  execution: InterruptibleExecution,
  signal?: AbortSignal,
  interruptionGraceMs = 2_000,
): Promise<TerminalExecutionResult> {
  let timer: NodeJS.Timeout | undefined
  let abortListener: (() => void) | undefined
  const interrupted = new Promise<'cancelled' | 'timed_out'>((resolve) => {
    if (signal?.aborted) {
      resolve('cancelled')
      return
    }
    if (signal) {
      abortListener = () => resolve('cancelled')
      signal.addEventListener('abort', abortListener, { once: true })
    }
    timer = setTimeout(() => resolve('timed_out'), execution.timeoutMs)
  })

  try {
    const outcome = await Promise.race([
      execution.completion.then((value) => ({
        kind: 'complete' as const,
        ...value,
      })),
      interrupted.then((kind) => ({ kind })),
    ])
    if (outcome.kind === 'complete') {
      return {
        exitCode: outcome.exitCode,
        output: outcome.output,
        outputTruncated: outcome.outputTruncated ?? false,
        cancelled: false,
        timedOut: false,
      }
    }

    execution.interrupt()
    const partial = await Promise.race([
      execution.completion,
      delay(interruptionGraceMs).then(() => ({
        exitCode: undefined,
        output: execution.partialOutput(),
      })),
    ])
    return {
      exitCode: partial.exitCode,
      output: partial.output,
      outputTruncated:
        ('outputTruncated' in partial
          ? partial.outputTruncated
          : execution.partialOutputTruncated?.()) ?? false,
      cancelled: outcome.kind === 'cancelled',
      timedOut: outcome.kind === 'timed_out',
    }
  } finally {
    if (timer) {
      clearTimeout(timer)
    }
    if (abortListener) {
      signal?.removeEventListener('abort', abortListener)
    }
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
  if (action.type === 'TerminalEnsure') {
    await facade.ensure(action.session_key, action.cwd, true)
    return { status: 'done' }
  }

  if (
    action.completion_marker &&
    (await facade.completionMarkerExists(action.completion_marker))
  ) {
    return {
      status: 'done',
      completion_marker_found: true,
    }
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
    timedOut: t('Command timed out after {0} ms', action.timeout_ms),
    noExitCode: t('Shell did not return an exit code'),
    exitCode: t('Command exited with code {0}', decision.exitCode),
    outputTruncated: t(
      'Terminal output was truncated before expected output could be verified: {0}',
      missing,
    ),
    outputMismatch: t('Expected terminal output not found: {0}', missing),
  })
}

/**
 * VS Code shell integration can treat `a; b` as two executions and resolve the
 * first one before `b` writes its output. A shell group keeps the whole check
 * line as one execution, so the run output is captured with the compile output.
 */
export function groupCompoundShellCommand(
  command: string,
  shell: string | undefined,
): string {
  if (!/[;&|]/.test(command)) {
    return command
  }
  switch (shell) {
    case 'pwsh':
    case 'powershell':
      return `& { ${command} }`
    case 'cmd':
      // cmd.exe has no `{...}` group; the course checks use PowerShell/POSIX.
      return command
    case 'fish':
      return `begin; ${command}; end`
    case 'csh':
      return `( ${command} )`
    default:
      return `{ ${command}; }`
  }
}

/**
 * Shell-integration output is unreliable for native child processes in some
 * shells. Challenge checks redirect to a file; the adapter reads it after the
 * terminal action and returns that as the check output.
 */
export function redirectShellOutput(
  command: string,
  shell: string | undefined,
  filePath: string,
): string {
  switch (shell) {
    case 'pwsh':
    case 'powershell':
      return `${command} *> '${filePath.replace(/'/g, "''")}'`
    case 'cmd':
      return `${command} > "${filePath.replace(/"/g, '""')}" 2>&1`
    default:
      return `${command} > '${filePath.replace(/'/g, "'\\''")}' 2>&1`
  }
}

type CaptureEncoding = 'utf8' | 'utf16le' | 'utf16be'

function captureEncoding(prefix: Buffer): {
  encoding: CaptureEncoding
  bomBytes: number
} {
  if (prefix[0] === 0xff && prefix[1] === 0xfe) {
    return { encoding: 'utf16le', bomBytes: 2 }
  }
  if (prefix[0] === 0xfe && prefix[1] === 0xff) {
    return { encoding: 'utf16be', bomBytes: 2 }
  }
  return { encoding: 'utf8', bomBytes: 0 }
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff
}

function trimUtf16Tail(text: string, startsAtFileStart: boolean): string {
  if (startsAtFileStart && text.startsWith('\uFEFF')) {
    text = text.slice(1)
  }
  if (text && isLowSurrogate(text.charCodeAt(0))) {
    text = text.slice(1)
  }
  if (text && isHighSurrogate(text.charCodeAt(text.length - 1))) {
    text = text.slice(0, -1)
  }
  return text
}

function decodeCapture(
  bytes: Buffer,
  encoding: CaptureEncoding,
  startsAtFileStart: boolean,
  maxBytes: number,
): string {
  if (encoding === 'utf16le') {
    return utf8Tail(
      trimUtf16Tail(bytes.toString('utf16le'), startsAtFileStart),
      maxBytes,
    )
  }
  if (encoding === 'utf16be') {
    const evenLength = bytes.length - (bytes.length % 2)
    const swapped = Buffer.allocUnsafe(evenLength)
    for (let index = 0; index < evenLength; index += 2) {
      swapped[index] = bytes[index + 1]
      swapped[index + 1] = bytes[index]
    }
    return utf8Tail(
      trimUtf16Tail(swapped.toString('utf16le'), startsAtFileStart),
      maxBytes,
    )
  }
  const bounded = utf8TailBytes(bytes, maxBytes)
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bounded)
  } catch {
    // cmd.exe redirects native output in the active console code page. GBK is
    // the common Chinese Windows code page; ASCII remains UTF-8-compatible.
    return utf8Tail(new TextDecoder('gbk').decode(bounded), maxBytes)
  }
}

export async function readBoundedFileTail(
  filePath: string,
  maxBytes: number,
): Promise<{ output: string; truncated: boolean }> {
  const handle = await open(filePath, 'r')
  try {
    const { size } = await handle.stat()
    const prefix = Buffer.alloc(Math.min(size, 4))
    if (prefix.length > 0) {
      await handle.read(prefix, 0, prefix.length, 0)
    }
    const { encoding, bomBytes } = captureEncoding(prefix)
    let length = Math.min(size, maxBytes + 4)
    let start = size - length
    if (encoding !== 'utf8' && start > 0) {
      if ((start - bomBytes) % 2 !== 0) {
        start -= 1
        length += 1
      }
    }
    const buffer = Buffer.alloc(length)
    const { bytesRead } = await handle.read(buffer, 0, length, start)
    return {
      output: decodeCapture(
        buffer.subarray(0, bytesRead),
        encoding,
        start === 0,
        maxBytes,
      ),
      truncated: size > maxBytes,
    }
  } finally {
    await handle.close()
  }
}

export function collectOutput(
  stream: AsyncIterable<string>,
): Promise<string> & OutputCollector {
  let chunks: string[] = []
  let bytes = 0
  let wasTruncated = false

  const text = (): string => chunks.join('')
  const trim = (): void => {
    while (bytes > MAX_TERMINAL_OUTPUT_BYTES && chunks.length > 0) {
      const first = chunks[0]
      const firstBytes = Buffer.byteLength(first, 'utf8')
      const excess = bytes - MAX_TERMINAL_OUTPUT_BYTES
      if (firstBytes <= excess) {
        chunks.shift()
        bytes -= firstBytes
        continue
      }
      const tail = utf8Tail(first, firstBytes - excess)
      const tailBytes = Buffer.byteLength(tail, 'utf8')
      chunks[0] = tail
      bytes -= firstBytes - tailBytes
    }
  }

  const promise = (async () => {
    for await (const chunk of stream) {
      chunks.push(chunk)
      bytes += Buffer.byteLength(chunk, 'utf8')
      if (bytes > MAX_TERMINAL_OUTPUT_BYTES) {
        wasTruncated = true
        trim()
      }
    }
    return text()
  })() as Promise<string> & OutputCollector
  promise.partial = text
  promise.truncated = () => wasTruncated
  return promise
}

function utf8TailBytes(bytes: Buffer, maxBytes: number): Buffer {
  if (bytes.length <= maxBytes) {
    return bytes
  }
  let start = bytes.length - maxBytes
  while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) {
    start += 1
  }
  if (bytes.length - start > maxBytes) {
    start += 1
    while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) {
      start += 1
    }
  }
  return bytes.subarray(start)
}

function utf8Tail(value: string, maxBytes: number): string {
  return utf8TailBytes(Buffer.from(value, 'utf8'), maxBytes).toString('utf8')
}

export function shellIntegrationUnavailable(): ApplyError {
  return new ApplyError(
    'shell_integration_unavailable',
    t(
      'VS Code Shell Integration is unavailable. Enable terminal.integrated.shellIntegration.enabled and use a supported shell, then select Recheck.',
    ),
  )
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds))
}
