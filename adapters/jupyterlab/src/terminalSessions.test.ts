import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  TerminalSessionManager,
  terminalCwdIsWithinResetPaths,
  terminalCwdsToDisposeUnderPaths,
} from './terminalSessions'

interface FakeTerminal {
  name: string
  cwd: string
  closed: boolean
}

function manager(
  created: FakeTerminal[],
  disposed: string[],
): TerminalSessionManager<FakeTerminal> {
  let sequence = 0
  return new TerminalSessionManager<FakeTerminal>({
    async create(sessionKey, cwdPath) {
      const terminal = {
        name: `${sessionKey}-${sequence++}`,
        cwd: cwdPath,
        closed: false,
      }
      created.push(terminal)
      return terminal
    },
    isClosed(terminal) {
      return terminal.closed
    },
    async dispose(terminal) {
      disposed.push(terminal.name)
      terminal.closed = true
    },
  })
}

test('reuses a session while its key and working directory are unchanged', async () => {
  const created: FakeTerminal[] = []
  const disposed: string[] = []
  const sessions = manager(created, disposed)

  const first = await sessions.ensure('unit::main', 'ch/unit')
  const second = await sessions.ensure('unit::main', 'ch/unit')

  assert.equal(first, second)
  assert.equal(created.length, 1)
  assert.deepEqual(disposed, [])
})

test('recreates a session when the terminal closed or the directory changed', async () => {
  const created: FakeTerminal[] = []
  const disposed: string[] = []
  const sessions = manager(created, disposed)

  const first = await sessions.ensure('unit::main', 'ch/unit')
  first.terminal.closed = true
  const second = await sessions.ensure('unit::main', 'ch/unit')
  assert.notEqual(second, first)
  assert.equal(created.length, 2)

  const third = await sessions.ensure('unit::main', 'ch/other')
  assert.notEqual(third, second)
  assert.equal(created.length, 3)
  assert.deepEqual(disposed, [first.terminal.name, second.terminal.name])
})

test('new_terminal replaces a live session with the same key', async () => {
  const created: FakeTerminal[] = []
  const disposed: string[] = []
  const sessions = manager(created, disposed)

  const first = await sessions.ensure('unit::main', 'ch/unit')
  const second = await sessions.ensure('unit::main', 'ch/unit', true)

  assert.notEqual(second, first)
  assert.deepEqual(disposed, [first.terminal.name])
  assert.equal(sessions.get('unit::main')?.terminal, second.terminal)
})

test('reset paths select session directories inside the workspace scope', () => {
  assert.equal(terminalCwdIsWithinResetPaths([], 'ch/unit'), true)
  assert.equal(terminalCwdIsWithinResetPaths(['.'], 'ch/unit'), true)
  assert.equal(terminalCwdIsWithinResetPaths(['ch/unit'], 'ch/unit'), true)
  assert.equal(
    terminalCwdIsWithinResetPaths(['ch/unit'], 'ch/unit/nested'),
    true,
  )
  assert.equal(terminalCwdIsWithinResetPaths(['ch/unit'], 'ch/other'), false)
  assert.equal(
    terminalCwdIsWithinResetPaths(['ch'], 'ch/unit', ['ch/unit']),
    false,
  )
})

test('only sessions under the reset paths and outside exclusions are closed', () => {
  const sessions = [
    { sessionKey: 'a', cwdPath: 'ch/unit' },
    { sessionKey: 'b', cwdPath: 'ch/unit/nested' },
    { sessionKey: 'c', cwdPath: 'ch/other' },
    { sessionKey: 'd', cwdPath: 'ch/unit/keep' },
  ]

  assert.deepEqual(
    terminalCwdsToDisposeUnderPaths(
      sessions,
      ['ch/unit'],
      ['ch/unit/keep'],
    ).map((entry) => entry.sessionKey),
    ['a', 'b'],
  )
  assert.deepEqual(
    terminalCwdsToDisposeUnderPaths(sessions, [], []).map(
      (entry) => entry.sessionKey,
    ),
    ['a', 'b', 'c', 'd'],
  )
})
