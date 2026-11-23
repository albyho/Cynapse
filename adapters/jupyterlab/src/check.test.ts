import assert from 'node:assert/strict'
import { test } from 'node:test'

import { JupyterLabFacadeError } from './actions'
import {
  applyCheck,
  createJupyterLabCheckHandler,
  dirtyNotebooksUnder,
  parseCheckPayload,
  resolveUnitCheckCwd,
  type CheckNotebookFacade,
} from './check'
import type { TerminalCommandAction } from './protocol'
import type { TerminalExecutionResult, TerminalFacade } from './terminal'

class FakeNotebooks implements CheckNotebookFacade {
  readonly calls: string[] = []
  dirty: string[] = []
  failure: JupyterLabFacadeError | undefined

  async dirtyNotebooks(): Promise<string[]> {
    this.calls.push('dirty')
    return this.dirty
  }

  async saveNotebook(path: string): Promise<void> {
    this.calls.push(`save:${path}`)
    if (this.failure) {
      throw this.failure
    }
  }
}

class FakeTerminal implements TerminalFacade {
  readonly calls: string[] = []
  result: TerminalExecutionResult = {
    exitCode: 0,
    output: 'ok\n',
    cancelled: false,
    timedOut: false,
  }

  async ensure(sessionKey: string, cwd: string): Promise<void> {
    this.calls.push(`ensure:${sessionKey}:${cwd}`)
  }

  async completionMarkerExists(marker: string): Promise<boolean> {
    this.calls.push(`marker:${marker}`)
    return false
  }

  async disposeUnderPaths(paths: string[]): Promise<void> {
    this.calls.push(`dispose:${paths.join(',')}`)
  }

  async execute(
    action: TerminalCommandAction,
  ): Promise<TerminalExecutionResult> {
    this.calls.push(
      `execute:${action.session_key}:${action.cwd}:${action.command}`,
    )
    return this.result
  }
}

function payload(
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    type: 'COMMAND',
    unit_path: 'ch/unit',
    session_key: 'ch/unit::main',
    command: 'python check.py',
    ...overrides,
  }
}

test('parses a complete COMMAND check payload', () => {
  assert.deepEqual(
    parseCheckPayload(
      payload({
        cwd: 'src',
        accepted_exit_codes: [0, 1],
        expected_output: ['ok'],
        timeout_ms: 120000,
        save_dirty: true,
      }),
    ),
    {
      type: 'COMMAND',
      unit_path: 'ch/unit',
      session_key: 'ch/unit::main',
      cwd: 'src',
      command: 'python check.py',
      accepted_exit_codes: [0, 1],
      expected_output: ['ok'],
      timeout_ms: 120000,
      save_dirty: true,
    },
  )
})

test('applies check defaults without inventing a session or command', () => {
  const check = parseCheckPayload(payload())

  assert.equal(check?.cwd, '.')
  assert.deepEqual(check?.accepted_exit_codes, [0])
  assert.deepEqual(check?.expected_output, [])
  assert.equal(check?.timeout_ms, 300000)
  assert.equal(check?.save_dirty, false)
})

test('rejects an invalid check payload', () => {
  assert.equal(parseCheckPayload({ type: 'COMMAND' }), undefined)
  assert.equal(parseCheckPayload(payload({ session_key: '' })), undefined)
  assert.equal(parseCheckPayload(payload({ command: '' })), undefined)
  assert.equal(parseCheckPayload(payload({ type: 'OTHER' })), undefined)
  assert.equal(
    parseCheckPayload(payload({ accepted_exit_codes: [] })),
    undefined,
  )
  assert.equal(parseCheckPayload(payload({ timeout_ms: 0 })), undefined)
  assert.equal(parseCheckPayload(payload({ save_dirty: 'yes' })), undefined)
  assert.equal(
    parseCheckPayload(payload({ unit_path: '../escape' })),
    undefined,
  )
})

test('resolves the check cwd inside the Unit Workspace only', () => {
  assert.equal(resolveUnitCheckCwd('ch/unit', '.'), 'ch/unit')
  assert.equal(resolveUnitCheckCwd('ch/unit', 'src'), 'ch/unit/src')
  assert.equal(resolveUnitCheckCwd('ch/unit', './src/'), 'ch/unit/src')
  assert.equal(resolveUnitCheckCwd('ch/unit', '..'), undefined)
  assert.equal(resolveUnitCheckCwd('ch/unit', '../other'), undefined)
  assert.equal(resolveUnitCheckCwd('ch/unit', '/absolute'), undefined)
  assert.equal(
    resolveUnitCheckCwd('ch/unit', 'C:\\absolute'), // test-isolation-allow: absolute-path fixture
    undefined,
  )
})

test('finds dirty Notebooks inside the Unit Workspace only', () => {
  const dirty = ['ch/unit/a.ipynb', 'ch/unit2/b.ipynb', 'ch/other/c.ipynb']

  assert.deepEqual(dirtyNotebooksUnder('ch/unit', dirty), ['ch/unit/a.ipynb'])
  assert.deepEqual(dirtyNotebooksUnder('ch', dirty), dirty)
})

test('a dirty Notebook returns workspace_dirty without running the command', async () => {
  const notebooks = new FakeNotebooks()
  notebooks.dirty = ['ch/unit/a.ipynb']
  notebooks.calls.length = 0
  const terminal = new FakeTerminal()

  const ack = await applyCheck(
    payload(),
    notebooks,
    terminal,
    new AbortController().signal,
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'workspace_dirty')
  assert.deepEqual(notebooks.calls, ['dirty'])
  assert.deepEqual(terminal.calls, [])
})

test('Save & Check saves the dirty Notebook before running the command', async () => {
  const notebooks = new FakeNotebooks()
  notebooks.dirty = ['ch/unit/a.ipynb', 'ch/unit/b.ipynb']
  const terminal = new FakeTerminal()

  const ack = await applyCheck(
    payload({ save_dirty: true }),
    notebooks,
    terminal,
    new AbortController().signal,
  )

  assert.equal(ack.status, 'done')
  assert.deepEqual(notebooks.calls, [
    'dirty',
    'save:ch/unit/a.ipynb',
    'save:ch/unit/b.ipynb',
  ])
  assert.deepEqual(terminal.calls, [
    'execute:ch/unit::main:ch/unit:python check.py',
  ])
})

test('a save conflict stops the check before the command runs', async () => {
  const notebooks = new FakeNotebooks()
  notebooks.dirty = ['ch/unit/a.ipynb']
  notebooks.failure = new JupyterLabFacadeError(
    'save_conflict',
    'Notebook changed on disk',
  )
  const terminal = new FakeTerminal()

  const ack = await applyCheck(
    payload({ save_dirty: true }),
    notebooks,
    terminal,
    new AbortController().signal,
  )

  assert.deepEqual(ack, {
    status: 'error',
    code: 'save_conflict',
    message: 'Notebook changed on disk',
  })
  assert.deepEqual(terminal.calls, [])
})

test('runs a clean check with the declared cwd and terminal fields', async () => {
  const notebooks = new FakeNotebooks()
  const terminal = new FakeTerminal()
  terminal.result = {
    exitCode: 0,
    output: 'ok\n',
    cancelled: false,
    timedOut: false,
  }

  const ack = await applyCheck(
    payload({
      cwd: 'src',
      accepted_exit_codes: [0, 3],
      expected_output: ['ok'],
      timeout_ms: 5000,
    }),
    notebooks,
    terminal,
    new AbortController().signal,
  )

  assert.deepEqual(ack, {
    status: 'done',
    exit_code: 0,
    output: 'ok\n',
    output_truncated: false,
  })
  assert.deepEqual(terminal.calls, [
    'execute:ch/unit::main:ch/unit/src:python check.py',
  ])
})

test('a check whose cwd escapes the Unit Workspace is rejected before the terminal', async () => {
  const notebooks = new FakeNotebooks()
  const terminal = new FakeTerminal()

  const ack = await applyCheck(
    payload({ cwd: '..' }),
    notebooks,
    terminal,
    new AbortController().signal,
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'invalid_check')
  assert.deepEqual(notebooks.calls, [])
  assert.deepEqual(terminal.calls, [])
})

test('the connection handler acknowledges an invalid check payload', async () => {
  const handler = createJupyterLabCheckHandler(
    new FakeNotebooks(),
    new FakeTerminal(),
  )

  const ack = await handler(
    {
      v: 2,
      id: '00000000-0000-4000-8000-000000000001',
      type: 'check',
      payload: { type: 'COMMAND' },
    },
    { signal: new AbortController().signal },
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'invalid_check')
})
