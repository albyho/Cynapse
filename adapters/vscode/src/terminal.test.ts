import assert from 'node:assert/strict'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'

import { ApplyError } from './apply'
import type { TerminalCommandAction } from '@cynapse/adapter-kit'
import {
  applyTerminalAction,
  awaitInterruptibleExecution,
  collectOutput,
  executeTerminalCommandInDirectory,
  groupCompoundShellCommand,
  MAX_TERMINAL_OUTPUT_BYTES,
  readBoundedFileTail,
  redirectShellOutput,
  type TerminalFacade,
} from './terminal'
import { terminalCwdIsWithinResetPaths } from './terminalSessions'

test('groups compound commands per shell so child output stays in one execution', () => {
  assert.equal(
    groupCompoundShellCommand('rustc hello.rs; ./hello', 'pwsh'),
    '& { rustc hello.rs; ./hello }',
  )
  assert.equal(
    groupCompoundShellCommand('rustc hello.rs; ./hello', 'powershell'),
    '& { rustc hello.rs; ./hello }',
  )
  assert.equal(
    groupCompoundShellCommand('rustc hello.rs; ./hello', 'bash'),
    '{ rustc hello.rs; ./hello; }',
  )
  assert.equal(
    groupCompoundShellCommand('rustc hello.rs; ./hello', 'fish'),
    'begin; rustc hello.rs; ./hello; end',
  )
  assert.equal(
    groupCompoundShellCommand('rustc hello.rs; ./hello', 'cmd'),
    'rustc hello.rs; ./hello',
  )
  assert.equal(
    groupCompoundShellCommand('rustc hello.rs', 'pwsh'),
    'rustc hello.rs',
  )
})

test('redirects compound command output to a file per shell', () => {
  assert.equal(
    redirectShellOutput(
      '& { rustc hello.rs; ./hello }',
      'pwsh',
      'C:\\tmp\\check.log', // test-isolation-allow: shell redirect fixture
    ),
    "& { rustc hello.rs; ./hello } *> 'C:\\tmp\\check.log'", // test-isolation-allow: shell redirect fixture
  )
  assert.equal(
    redirectShellOutput(
      '{ rustc hello.rs; ./hello; }',
      'bash',
      '/tmp/check.log', // test-isolation-allow: shell redirect fixture
    ),
    "{ rustc hello.rs; ./hello; } > '/tmp/check.log' 2>&1", // test-isolation-allow: shell redirect fixture
  )
  assert.equal(
    redirectShellOutput(
      'rustc hello.rs; ./hello',
      'cmd',
      'C:\\tmp\\check.log', // test-isolation-allow: shell redirect fixture
    ),
    'rustc hello.rs; ./hello > "C:\\tmp\\check.log" 2>&1', // test-isolation-allow: shell redirect fixture
  )
  assert.equal(
    redirectShellOutput('rustc hello.rs', 'pwsh', 'C:\\tmp\\check.log'), // test-isolation-allow: shell redirect fixture
    "rustc hello.rs *> 'C:\\tmp\\check.log'", // test-isolation-allow: shell redirect fixture
  )
})

function command(
  overrides: Partial<TerminalCommandAction> = {},
): TerminalCommandAction {
  return {
    type: 'TerminalCommand',
    session_key: 'main',
    cwd: 'hello_world',
    command: 'rustc main.rs',
    accepted_exit_codes: [0],
    expected_output: [],
    timeout_ms: 300_000,
    ...overrides,
  }
}

function facade(execute: TerminalFacade['execute']): TerminalFacade {
  return {
    async ensure() {},
    async completionMarkerExists() {
      return false
    },
    async disposeUnderPaths() {},
    execute,
  }
}

test('accepts normalized output after stripping ANSI codes', async () => {
  const ack = await applyTerminalAction(
    command({ expected_output: ['Hello,\nworld!'] }),
    facade(async () => ({
      exitCode: 0,
      output: '\u001b[32mHello,\r\nworld!\u001b[0m\r\n',
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'done')
  assert.equal(ack.exit_code, 0)
  assert.equal(ack.output, 'Hello,\nworld!\n')
})

test('keeps the output tail with an explicit truncation marker', async () => {
  const output = `HEAD\n${'x'.repeat(MAX_TERMINAL_OUTPUT_BYTES)}\nTAIL`
  const ack = await applyTerminalAction(
    command(),
    facade(async () => ({
      exitCode: 0,
      output,
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'done')
  assert.equal(ack.output_truncated, true)
  assert.match(ack.output ?? '', /^\[Cynapse: output truncated\]\n/)
  assert.ok(!(ack.output ?? '').includes('HEAD'))
  assert.ok((ack.output ?? '').endsWith('TAIL'))
})

test('reads only a bounded tail from a large capture file', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'cynapse-terminal-'))
  try {
    const capture = path.join(directory, 'capture.log')
    await writeFile(
      capture,
      `HEAD\n${'x'.repeat(MAX_TERMINAL_OUTPUT_BYTES * 2)}\nTAIL`,
    )

    const tail = await readBoundedFileTail(capture, MAX_TERMINAL_OUTPUT_BYTES)

    assert.equal(tail.truncated, true)
    assert.ok(
      Buffer.byteLength(tail.output, 'utf8') <= MAX_TERMINAL_OUTPUT_BYTES,
    )
    assert.ok(!tail.output.includes('HEAD'))
    assert.ok(tail.output.endsWith('TAIL'))
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('reports truncation when a real capture tail lacks expected output', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'cynapse-terminal-'))
  try {
    const capture = path.join(directory, 'capture.log')
    await writeFile(
      capture,
      `EXPECTED\n${'x'.repeat(MAX_TERMINAL_OUTPUT_BYTES * 2)}\nTAIL`,
    )
    const tail = await readBoundedFileTail(capture, MAX_TERMINAL_OUTPUT_BYTES)
    const ack = await applyTerminalAction(
      command({ expected_output: ['EXPECTED'] }),
      facade(async () => ({
        exitCode: 0,
        output: tail.output,
        outputTruncated: tail.truncated,
        cancelled: false,
        timedOut: false,
      })),
    )

    assert.equal(ack.status, 'error')
    assert.equal(ack.code, 'terminal_output_truncated')
    assert.equal(ack.output_truncated, true)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('keeps a UTF-8 character intact at the capture tail boundary', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'cynapse-terminal-'))
  try {
    const capture = path.join(directory, 'capture.log')
    await writeFile(capture, '中'.repeat(MAX_TERMINAL_OUTPUT_BYTES) + 'TAIL')

    const tail = await readBoundedFileTail(capture, MAX_TERMINAL_OUTPUT_BYTES)

    assert.equal(tail.truncated, true)
    assert.ok(!tail.output.includes('\uFFFD'))
    assert.ok(tail.output.endsWith('TAIL'))
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('reads a UTF-16LE capture written by Windows PowerShell', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'cynapse-terminal-'))
  try {
    const capture = path.join(directory, 'capture.log')
    await writeFile(
      capture,
      Buffer.concat([
        Buffer.from([0xff, 0xfe]),
        Buffer.from('OK\r\n', 'utf16le'),
      ]),
    )

    const tail = await readBoundedFileTail(capture, MAX_TERMINAL_OUTPUT_BYTES)

    assert.equal(tail.output, 'OK\r\n')
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('keeps a bounded UTF-16LE capture tail aligned', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'cynapse-terminal-'))
  try {
    const capture = path.join(directory, 'capture.log')
    const content = `HEAD\n${'x'.repeat(MAX_TERMINAL_OUTPUT_BYTES * 2)}\n你好`
    await writeFile(
      capture,
      Buffer.concat([
        Buffer.from([0xff, 0xfe]),
        Buffer.from(content, 'utf16le'),
      ]),
    )

    const tail = await readBoundedFileTail(capture, MAX_TERMINAL_OUTPUT_BYTES)

    assert.equal(tail.truncated, true)
    assert.ok(
      Buffer.byteLength(tail.output, 'utf8') <= MAX_TERMINAL_OUTPUT_BYTES,
    )
    assert.ok(!tail.output.includes('\uFEFF'))
    assert.ok(!tail.output.includes('\uFFFD'))
    assert.ok(tail.output.endsWith('你好'))
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('decodes a GBK cmd capture when it is not valid UTF-8', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'cynapse-terminal-'))
  try {
    const capture = path.join(directory, 'capture.log')
    await writeFile(capture, Buffer.from([0xc4, 0xe3, 0xba, 0xc3, 0x0d, 0x0a]))

    const tail = await readBoundedFileTail(capture, MAX_TERMINAL_OUTPUT_BYTES)

    assert.equal(tail.output, '你好\r\n')
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('collects only a bounded in-memory output tail while streaming', async () => {
  async function* output() {
    yield 'HEAD\n'
    for (let index = 0; index < 12; index += 1) {
      yield 'x'.repeat(1024 * 1024)
    }
    yield '\nTAIL'
  }

  const collector = collectOutput(output())
  const result = await collector

  assert.equal(collector.truncated(), true)
  assert.ok(Buffer.byteLength(result, 'utf8') <= MAX_TERMINAL_OUTPUT_BYTES)
  assert.ok(!result.includes('HEAD'))
  assert.ok(result.endsWith('\nTAIL'))
})

test('matches expected output against the full source before bounding the ack', async () => {
  const output = `EXPECTED\n${'x'.repeat(MAX_TERMINAL_OUTPUT_BYTES * 2)}\nTAIL`
  const ack = await applyTerminalAction(
    command({ expected_output: ['EXPECTED'] }),
    facade(async () => ({
      exitCode: 0,
      output,
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'done')
  assert.equal(ack.output_truncated, true)
  assert.ok(!(ack.output ?? '').includes('EXPECTED'))
})

test('reports truncated output instead of a mismatch when only the tail was retained', async () => {
  const ack = await applyTerminalAction(
    command({ expected_output: ['EXPECTED'] }),
    facade(async () => ({
      exitCode: 0,
      output: `${'x'.repeat(MAX_TERMINAL_OUTPUT_BYTES * 2)}\nTAIL`,
      outputTruncated: true,
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'terminal_output_truncated')
  assert.equal(ack.output_truncated, true)
})

test('rejects a non-zero exit even when expected output is present', async () => {
  const ack = await applyTerminalAction(
    command({ expected_output: ['failure detail'] }),
    facade(async () => ({
      exitCode: 1,
      output: 'failure detail\n',
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'terminal_exit_code_rejected')
  assert.equal(ack.exit_code, 1)
})

test('rejects output that does not contain every expected substring', async () => {
  const ack = await applyTerminalAction(
    command({ expected_output: ['Hello, world!', 'second line'] }),
    facade(async () => ({
      exitCode: 0,
      output: 'Hello, world!\n',
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'terminal_output_mismatch')
  assert.match(ack.message ?? '', /second line/)
})

test('accepts cargo run when shell integration does not retain child output', async () => {
  const ack = await applyTerminalAction(
    command({
      command: 'cargo run',
      expected_output: [],
    }),
    facade(async () => ({
      exitCode: 0,
      output: '    Finished `dev` profile [unoptimized + debuginfo]\n(base) ',
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'done')
  assert.equal(ack.exit_code, 0)
})

test('accepts an explicitly allowed non-zero exit with expected error output', async () => {
  const ack = await applyTerminalAction(
    command({
      command: 'rustc main.rs',
      accepted_exit_codes: [1],
      expected_output: ['cannot assign twice to immutable variable'],
    }),
    facade(async () => ({
      exitCode: 1,
      output:
        '\u001b[31merror[E0384]: cannot assign twice to immutable variable\u001b[0m\r\n',
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'done')
  assert.equal(ack.exit_code, 1)
  assert.match(ack.output ?? '', /cannot assign twice/)
})

test('matches expected output case-sensitively', async () => {
  const ack = await applyTerminalAction(
    command({ expected_output: ['Hello'] }),
    facade(async () => ({
      exitCode: 0,
      output: 'hello\n',
      cancelled: false,
      timedOut: false,
    })),
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'terminal_output_mismatch')
})

test('does not rerun cargo new when Cargo.toml already exists', async () => {
  let executions = 0
  const ack = await applyTerminalAction(
    command({
      command: 'cargo new hello_cargo',
      completion_marker: 'hello_cargo/Cargo.toml',
    }),
    {
      async ensure() {},
      async completionMarkerExists(marker) {
        assert.equal(marker, 'hello_cargo/Cargo.toml')
        return true
      },
      async disposeUnderPaths() {},
      async execute() {
        executions += 1
        throw new Error('command should not execute')
      },
    },
  )

  assert.equal(ack.status, 'done')
  assert.equal(ack.completion_marker_found, true)
  assert.equal(executions, 0)
})

test('reveals a dedicated terminal without taking focus', async () => {
  const calls: Array<{
    sessionKey: string
    cwd: string
    preserveFocus: boolean
  }> = []
  const ack = await applyTerminalAction(
    {
      type: 'TerminalEnsure',
      session_key: 'main',
      cwd: 'hello_world',
    },
    {
      async ensure(sessionKey, cwd, preserveFocus) {
        calls.push({ sessionKey, cwd, preserveFocus })
      },
      async completionMarkerExists() {
        return false
      },
      async disposeUnderPaths() {},
      async execute() {
        throw new Error('not used')
      },
    },
  )

  assert.equal(ack.status, 'done')
  assert.deepEqual(calls, [
    {
      sessionKey: 'main',
      cwd: 'hello_world',
      preserveFocus: true,
    },
  ])
})

test('turns a timed-out command into a retryable cancelled acknowledgement', async () => {
  const ack = await applyTerminalAction(
    command({ timeout_ms: 25 }),
    facade(async () => ({
      exitCode: undefined,
      output: 'partial output',
      cancelled: false,
      timedOut: true,
    })),
  )

  assert.equal(ack.status, 'cancelled')
  assert.equal(ack.code, 'terminal_timeout')
  assert.equal(ack.timed_out, true)
  assert.equal(ack.output, 'partial output')
})

test('sends Ctrl+C and keeps partial output when a command is paused', async () => {
  const controller = new AbortController()
  let interrupted = 0
  const running = awaitInterruptibleExecution(
    {
      completion: new Promise(() => {}),
      partialOutput: () => 'partial output',
      interrupt() {
        interrupted += 1
      },
      timeoutMs: 5_000,
    },
    controller.signal,
    20,
  )

  controller.abort()
  const result = await running

  assert.equal(result.cancelled, true)
  assert.equal(result.timedOut, false)
  assert.equal(result.output, 'partial output')
  assert.equal(interrupted, 1)
})

test('sends Ctrl+C and keeps partial output when a command times out', async () => {
  let interrupted = 0
  const result = await awaitInterruptibleExecution(
    {
      completion: new Promise(() => {}),
      partialOutput: () => 'timed partial output',
      interrupt() {
        interrupted += 1
      },
      timeoutMs: 5,
    },
    undefined,
    20,
  )

  assert.equal(result.cancelled, false)
  assert.equal(result.timedOut, true)
  assert.equal(result.output, 'timed partial output')
  assert.equal(interrupted, 1)
})

test('runs the directory change and course command as separate executions', async () => {
  const calls: Array<{ command: string; timeoutMs: number }> = []
  const currentDirectory = path.resolve('test-workspace/course')
  const targetDirectory = path.join(currentDirectory, "learner's work")
  const result = await executeTerminalCommandInDirectory({
    currentDirectory,
    targetDirectory,
    shell: 'pwsh',
    command: 'rustc main.rs',
    timeoutMs: 300_000,
    async execute(command, timeoutMs) {
      calls.push({ command, timeoutMs })
      return {
        exitCode: 0,
        output: command === 'rustc main.rs' ? 'Hello, world!\n' : '',
        cancelled: false,
        timedOut: false,
      }
    },
  })

  assert.deepEqual(calls, [
    {
      command: `cd -- '${targetDirectory.replaceAll("'", "''")}'`,
      timeoutMs: 5_000,
    },
    { command: 'rustc main.rs', timeoutMs: 300_000 },
  ])
  assert.equal(result.output, 'Hello, world!\n')
})

test('does not run the course command when directory change fails', async () => {
  const calls: string[] = []
  const currentDirectory = path.resolve('test-workspace/course')
  const targetDirectory = path.resolve('test-workspace/missing')

  await assert.rejects(
    executeTerminalCommandInDirectory({
      currentDirectory,
      targetDirectory,
      shell: 'pwsh',
      command: 'rustc main.rs',
      timeoutMs: 300_000,
      async execute(command) {
        calls.push(command)
        return {
          exitCode: 1,
          output: 'path not found',
          cancelled: false,
          timedOut: false,
        }
      },
    }),
    (error: unknown) =>
      error instanceof ApplyError &&
      error.code === 'terminal_cwd_change_failed',
  )

  assert.deepEqual(calls, [`cd -- '${targetDirectory.replaceAll("'", "''")}'`])
})

test('matches a terminal cwd that is inside a workspace reset path', () => {
  const workspace = path.resolve('test-workspace')

  assert.equal(
    terminalCwdIsWithinResetPaths(
      workspace,
      ['hello_world'],
      path.join(workspace, 'hello_world'),
    ),
    true,
  )
  assert.equal(
    terminalCwdIsWithinResetPaths(
      workspace,
      ['hello_world'],
      path.join(workspace, 'hello_world', 'nested'),
    ),
    true,
  )
  assert.equal(
    terminalCwdIsWithinResetPaths(
      workspace,
      ['hello_world'],
      path.join(workspace, 'hello_world_other'),
    ),
    false,
  )
})
