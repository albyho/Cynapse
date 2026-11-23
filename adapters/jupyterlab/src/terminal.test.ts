import assert from 'node:assert/strict'
import { test } from 'node:test'

import type { TerminalCommandAction } from './protocol'
import { parseTerminalAction } from './protocol'
import {
  applyTerminalAction,
  boundTerminalOutput,
  MAX_TERMINAL_OUTPUT_BYTES,
  type TerminalExecutionResult,
  type TerminalFacade,
} from './terminal'

function command(
  overrides: Partial<TerminalCommandAction> = {},
): TerminalCommandAction {
  return {
    type: 'TerminalCommand',
    session_key: 'main',
    cwd: 'unit',
    command: 'python --version',
    accepted_exit_codes: [0],
    expected_output: [],
    timeout_ms: 300_000,
    ...overrides,
  }
}

function facade(
  returns: Partial<TerminalExecutionResult> = {},
  calls: string[] = [],
): TerminalFacade {
  return {
    async ensure(sessionKey, cwd, newTerminal) {
      calls.push(`ensure:${sessionKey}:${cwd}:${newTerminal ?? false}`)
    },
    async completionMarkerExists(marker) {
      calls.push(`marker:${marker}`)
      return false
    },
    async disposeUnderPaths(paths, excludePaths) {
      calls.push(`dispose:${paths.join(',')}:${(excludePaths ?? []).join(',')}`)
    },
    async execute(action) {
      calls.push(
        `execute:${action.session_key}:${action.cwd}:${action.command}:${action.timeout_ms}`,
      )
      return {
        exitCode: 0,
        output: '',
        cancelled: false,
        timedOut: false,
        ...returns,
      }
    },
  }
}

test('parses terminal actions with the shared defaults', () => {
  assert.deepEqual(
    parseTerminalAction({
      type: 'TerminalEnsure',
      session_key: 'unit::main',
      cwd: 'unit',
    }),
    { type: 'TerminalEnsure', session_key: 'unit::main', cwd: 'unit' },
  )
  assert.deepEqual(
    parseTerminalAction({
      type: 'TerminalCommand',
      session_key: 'unit::main',
      cwd: 'unit',
      command: 'python --version',
    }),
    {
      type: 'TerminalCommand',
      session_key: 'unit::main',
      cwd: 'unit',
      command: 'python --version',
      accepted_exit_codes: [0],
      expected_output: [],
      timeout_ms: 300_000,
    },
  )
})

test('rejects malformed terminal actions and actions owned by other Adapters', () => {
  const invalid = [
    { type: 'TerminalEnsure', session_key: '', cwd: 'unit' },
    { type: 'TerminalEnsure', session_key: 'main', cwd: '' },
    { type: 'TerminalCommand', session_key: 'main', cwd: 'unit', command: '' },
    {
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: 'unit',
      command: 'x',
      timeout_ms: 0,
    },
    {
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: 'unit',
      command: 'x',
      accepted_exit_codes: [],
    },
    {
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: 'unit',
      command: 'x',
      expected_output: [''],
    },
    {
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: 'unit',
      command: 'x',
      accepted_exit_codes: [1.5],
    },
    { type: 'TerminalCommand', session_key: 'main', command: 'x' },
    {
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    },
  ]

  for (const payload of invalid) {
    assert.equal(
      parseTerminalAction(payload),
      undefined,
      JSON.stringify(payload),
    )
  }
})

test('TerminalEnsure creates or reuses the session and acknowledges', async () => {
  const calls: string[] = []
  const ack = await applyTerminalAction(
    { type: 'TerminalEnsure', session_key: 'unit::main', cwd: 'unit' },
    facade({}, calls),
  )

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(calls, ['ensure:unit::main:unit:false'])
})

test('TerminalCommand returns the exit code and output on success', async () => {
  const calls: string[] = []
  const ack = await applyTerminalAction(
    command({ expected_output: ['Python 3.13'] }),
    facade({ exitCode: 0, output: '\u001b[32mPython 3.13.0\r\n' }, calls),
  )

  assert.deepEqual(ack, {
    status: 'done',
    exit_code: 0,
    output: 'Python 3.13.0\n',
    output_truncated: false,
  })
  assert.deepEqual(calls, ['execute:main:unit:python --version:300000'])
})

test('a completion marker proves a replay-safe command already ran', async () => {
  const ack = await applyTerminalAction(
    command({ completion_marker: 'unit/done.txt' }),
    {
      ...facade(),
      async completionMarkerExists(marker) {
        return marker === 'unit/done.txt'
      },
      async execute() {
        throw new Error('the command must not run again')
      },
    },
  )

  assert.deepEqual(ack, { status: 'done', completion_marker_found: true })
})

test('a rejected exit code reports the exit code and output', async () => {
  const ack = await applyTerminalAction(
    command(),
    facade({ exitCode: 1, output: 'AssertionError\n' }),
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'terminal_exit_code_rejected')
  assert.equal(ack.exit_code, 1)
  assert.equal(ack.output, 'AssertionError\n')
})

test('missing expected output is a mismatch, or truncation when the tail was cut', async () => {
  const mismatch = await applyTerminalAction(
    command({ expected_output: ['all tests passed'] }),
    facade({ exitCode: 0, output: '2 failed\n' }),
  )
  assert.equal(mismatch.code, 'terminal_output_mismatch')
  assert.equal(mismatch.status, 'error')

  const truncated = await applyTerminalAction(
    command({ expected_output: ['all tests passed'] }),
    facade({ exitCode: 0, output: 'tail\n', outputTruncated: true }),
  )
  assert.equal(truncated.code, 'terminal_output_truncated')
  assert.equal(truncated.output_truncated, true)
})

test('a terminal timeout is reported as a timed-out cancellation', async () => {
  const ack = await applyTerminalAction(
    command({ timeout_ms: 1000 }),
    facade({ exitCode: undefined, output: 'partial\n', timedOut: true }),
  )

  assert.deepEqual(ack, {
    status: 'cancelled',
    code: 'terminal_timeout',
    timed_out: true,
    message: 'Command timed out after 1000 ms',
    output: 'partial\n',
    output_truncated: false,
  })
})

test('cancelling the active command returns cancelled', async () => {
  const controller = new AbortController()
  controller.abort()

  assert.deepEqual(
    await applyTerminalAction(command(), facade(), controller.signal),
    { status: 'cancelled' },
  )
})

test('an environment check ensures the terminal, runs each requirement, then cleans up', async () => {
  const calls: string[] = []
  const terminal = facade({ exitCode: 0, output: 'Python 3.13.0\n' }, calls)

  assert.deepEqual(
    await applyTerminalAction(
      { type: 'TerminalEnsure', session_key: 'main', cwd: 'ch/unit' },
      terminal,
    ),
    { status: 'done' },
  )
  for (const requirement of ['python --version', 'git --version']) {
    assert.equal(
      (
        await applyTerminalAction(
          command({
            session_key: 'main',
            cwd: 'ch/unit',
            command: requirement,
            timeout_ms: 30_000,
          }),
          terminal,
        )
      ).status,
      'done',
    )
  }
  await terminal.disposeUnderPaths([])

  assert.deepEqual(calls, [
    'ensure:main:ch/unit:false',
    'execute:main:ch/unit:python --version:30000',
    'execute:main:ch/unit:git --version:30000',
    'dispose::',
  ])
})

test('bounded output keeps the tail and marks truncation', () => {
  assert.deepEqual(boundTerminalOutput('short'), {
    output: 'short',
    truncated: false,
  })
  const long = `${'x'.repeat(MAX_TERMINAL_OUTPUT_BYTES)}TAIL`
  const bounded = boundTerminalOutput(long)
  assert.equal(bounded.truncated, true)
  assert.ok(bounded.output.startsWith('[Cynapse: output truncated]\n'))
  assert.ok(bounded.output.endsWith('TAIL'))
})
