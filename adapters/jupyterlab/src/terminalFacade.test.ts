import assert from 'node:assert/strict'
import { test as nodeTest } from 'node:test'

import type { TerminalCommandAction } from './protocol'
import type { TerminalConnection, TerminalHost } from './terminalFacade'
import { JupyterServerTerminalFacade } from './terminalFacade'
import { TerminalFacadeError } from './terminal'

/** A missing fake output must fail the test instead of hanging the runner. */
function test(name: string, run: () => Promise<void> | void): void {
  nodeTest(name, { timeout: 5_000 }, run)
}

class FakeConnection implements TerminalConnection {
  closed = false
  readonly sent: string[] = []
  private readonly outputListeners = new Set<(data: string) => void>()
  private readonly closeListeners = new Set<() => void>()

  constructor(readonly name: string) {}

  send(data: string): void {
    this.sent.push(data)
  }

  onOutput(listener: (data: string) => void): () => void {
    this.outputListeners.add(listener)
    return () => this.outputListeners.delete(listener)
  }

  onClose(listener: () => void): () => void {
    this.closeListeners.add(listener)
    return () => this.closeListeners.delete(listener)
  }

  emit(data: string): void {
    for (const listener of [...this.outputListeners]) {
      listener(data)
    }
  }

  close(): void {
    this.closed = true
    for (const listener of [...this.closeListeners]) {
      listener()
    }
  }

  dispose(): void {
    this.closed = true
  }
}

class FakeTerminalHost implements TerminalHost {
  readonly createdCwds: string[] = []
  readonly connections: FakeConnection[] = []
  readonly deleted: string[] = []
  readonly files = new Set<string>()
  private sequence = 0

  async create(cwd: string): Promise<TerminalConnection> {
    this.createdCwds.push(cwd)
    const connection = new FakeConnection(`terminal-${this.sequence++}`)
    this.connections.push(connection)
    return connection
  }

  async delete(name: string): Promise<void> {
    this.deleted.push(name)
  }

  async fileExists(path: string): Promise<boolean> {
    return this.files.has(path)
  }
}

function command(
  overrides: Partial<TerminalCommandAction> = {},
): TerminalCommandAction {
  return {
    type: 'TerminalCommand',
    session_key: 'unit::main',
    cwd: 'ch/unit',
    command: 'python --version',
    accepted_exit_codes: [0],
    expected_output: [],
    timeout_ms: 300_000,
    ...overrides,
  }
}

function facade(host: FakeTerminalHost): JupyterServerTerminalFacade {
  return new JupyterServerTerminalFacade(host, {
    shell: 'posix',
    interruptionGraceMs: 10,
  })
}

test('reuses a terminal and recreates a closed or moved session', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)

  await terminal.ensure('unit::main', 'ch/unit')
  await terminal.ensure('unit::main', 'ch/unit')
  assert.deepEqual(host.createdCwds, ['ch/unit'])

  host.connections[0].closed = true
  await terminal.ensure('unit::main', 'ch/unit')
  assert.deepEqual(host.createdCwds, ['ch/unit', 'ch/unit'])
  assert.deepEqual(host.deleted, ['terminal-0'])

  await terminal.ensure('unit::main', 'ch/other')
  assert.deepEqual(host.createdCwds, ['ch/unit', 'ch/unit', 'ch/other'])
  assert.deepEqual(host.deleted, ['terminal-0', 'terminal-1'])
})

test('exposes the completion marker through the host file view', async () => {
  const host = new FakeTerminalHost()
  host.files.add('ch/unit/done.txt')
  const terminal = facade(host)

  assert.equal(await terminal.completionMarkerExists('ch/unit/done.txt'), true)
  assert.equal(await terminal.completionMarkerExists('ch/unit/missing'), false)
})

test('runs a command in the session and strips the shell echo and markers', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)
  const pending = terminal.execute(command())

  const connection = host.connections[0]
  await waitFor(() => connection.sent.length === 1)
  assert.ok(connection.sent[0].includes('python --version'))
  assert.ok(connection.sent[0].includes('__CYNAPSE_BEGIN__'))
  assert.ok(connection.sent[0].endsWith('\r'))

  connection.emit(
    "sh-5.2$ printf '\\n__CYNAPSE_BEGIN__\\n'; python --version; cynapse_code=$?; printf '\\n__CYNAPSE_EXIT__:%s\\n' \"$cynapse_code\"\r\n" +
      '__CYNAPSE_BEGIN__\r\n' +
      '\u001b[32mPython 3.13.0\u001b[0m\r\n' +
      '\r\n__CYNAPSE_EXIT__:0\r\n' +
      'sh-5.2$ ',
  )

  assert.deepEqual(await pending, {
    exitCode: 0,
    output: 'Python 3.13.0\n',
    outputTruncated: false,
    cancelled: false,
    timedOut: false,
  })
})

test('waits for the whole exit code when it arrives in two chunks', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)
  const pending = terminal.execute(command({ accepted_exit_codes: [127] }))

  const connection = host.connections[0]
  await waitFor(() => connection.sent.length === 1)
  connection.emit(
    '__CYNAPSE_BEGIN__\r\nsh: python: command not found\r\n__CYNAPSE_EXIT__:1',
  )
  connection.emit('27\r\nsh-5.2$ ')

  const result = await pending
  assert.equal(result.exitCode, 127)
  assert.equal(result.timedOut, false)
})

test('closes only the sessions named by a session-scoped cleanup', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)
  await terminal.ensure('main', 'ch/a')
  await terminal.ensure('environment', '.')
  const kept = host.connections[0]
  const closed = host.connections[1]

  await terminal.disposeUnderPaths([], [], ['environment'])

  assert.equal(closed.closed, true)
  assert.equal(kept.closed, false)
  assert.deepEqual(host.deleted, [closed.name])
})

test('keeps the bounded tail when a command floods the terminal', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)
  const pending = terminal.execute(command({ command: 'flood' }))

  const connection = host.connections[0]
  await waitFor(() => connection.sent.length === 1)
  connection.emit(`${'x'.repeat(300_000)}TAIL__CYNAPSE_EXIT__:0\n`)

  const result = await pending
  assert.equal(result.outputTruncated, true)
  assert.ok(result.output.endsWith('TAIL'))
  assert.equal(result.exitCode, 0)
})

test('interrupts a timed-out command, keeps partial output, and closes the session', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)
  const pending = terminal.execute(command({ timeout_ms: 20 }))

  const connection = host.connections[0]
  await waitFor(() => connection.sent.length === 1)
  connection.emit('__CYNAPSE_BEGIN__\npartial output\n')

  const result = await pending
  assert.equal(result.timedOut, true)
  assert.equal(result.cancelled, false)
  assert.equal(result.output, 'partial output\n')
  assert.equal(connection.sent.at(-1), '\u0003')
  assert.deepEqual(host.deleted, [connection.name])
})

test('cancelling a running command interrupts it too', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)
  const controller = new AbortController()
  const pending = terminal.execute(command(), controller.signal)

  const connection = host.connections[0]
  await waitFor(() => connection.sent.length === 1)
  controller.abort()

  const result = await pending
  assert.equal(result.cancelled, true)
  assert.equal(connection.sent.at(-1), '\u0003')
  assert.deepEqual(host.deleted, [connection.name])
})

test('a terminal that closes during a command is a typed session failure', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)
  const pending = terminal.execute(command())

  const connection = host.connections[0]
  await waitFor(() => connection.sent.length === 1)
  connection.close()

  await assert.rejects(
    pending,
    (error: unknown) =>
      error instanceof TerminalFacadeError &&
      error.code === 'terminal_session_unavailable',
  )
})

test('reset paths close only matching sessions and preserve exclusions', async () => {
  const host = new FakeTerminalHost()
  const terminal = facade(host)

  await terminal.ensure('a', 'ch/unit')
  await terminal.ensure('b', 'ch/unit/nested')
  await terminal.ensure('c', 'ch/other')
  await terminal.ensure('d', 'ch/unit/keep')

  await terminal.disposeUnderPaths(['ch/unit'], ['ch/unit/keep'])

  assert.deepEqual(host.deleted, ['terminal-0', 'terminal-1'])
  const created = host.createdCwds.length
  await terminal.ensure('c', 'ch/other')
  await terminal.ensure('d', 'ch/unit/keep')
  assert.equal(host.createdCwds.length, created)
})

async function waitFor(
  predicate: () => boolean,
  timeoutMs = 1000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() >= deadline) {
      throw new Error('timed out waiting for the fake terminal')
    }
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
}
