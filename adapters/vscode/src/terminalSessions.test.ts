import assert from 'node:assert/strict'
import * as fs from 'node:fs/promises'
import * as os from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'

import { ApplyError } from './applyModel'
import {
  filterSessionEntries,
  resolveTerminalDirectory,
  sameTerminalDirectory,
  TerminalSessionManager,
  terminalBelongsToWorkspace,
  terminalDirectoryCommand,
  terminalsToDisposeUnderPaths,
  type TerminalSessionHost,
} from './terminalSessions'

interface FakeTerminal {
  id: number
  closed: boolean
}

const workspaceRoot = path.resolve('test-workspace')
const terminalOne = path.join(workspaceRoot, 'one')
const terminalTwo = path.join(workspaceRoot, 'two')

function createHost(): {
  host: TerminalSessionHost<FakeTerminal>
  created: Array<{ sessionKey: string; cwdPath: string }>
} {
  const created: Array<{ sessionKey: string; cwdPath: string }> = []
  let nextId = 1
  return {
    created,
    host: {
      create(sessionKey, cwdPath) {
        created.push({ sessionKey, cwdPath })
        return { id: nextId++, closed: false }
      },
      isClosed(terminal) {
        return terminal.closed
      },
      dispose() {},
      show() {},
      async waitUntilReady() {},
    },
  }
}

test('reuses one terminal when the same session changes working directory', async () => {
  const { host, created } = createHost()
  const sessions = new TerminalSessionManager(host)

  const first = (
    await sessions.ensure({
      sessionKey: 'main',
      workspaceRoot,
      cwdPath: terminalOne,
      preserveFocus: true,
    })
  ).entry.terminal
  const second = (
    await sessions.ensure({
      sessionKey: 'main',
      workspaceRoot,
      cwdPath: terminalTwo,
      preserveFocus: true,
    })
  ).entry.terminal

  assert.equal(second, first)
  assert.deepEqual(created, [{ sessionKey: 'main', cwdPath: terminalOne }])
  assert.equal(sessions.get('main')?.cwdPath, terminalTwo)
})

test('creates separate terminals only for separate session keys', async () => {
  const { host, created } = createHost()
  const sessions = new TerminalSessionManager(host)

  await sessions.ensure({
    sessionKey: 'main',
    workspaceRoot,
    cwdPath: workspaceRoot,
    preserveFocus: true,
  })
  await sessions.ensure({
    sessionKey: 'explorer',
    workspaceRoot,
    cwdPath: workspaceRoot,
    preserveFocus: true,
  })

  assert.deepEqual(
    created.map((entry) => entry.sessionKey),
    ['main', 'explorer'],
  )
})

test('creates a replacement terminal only when the action requests it', async () => {
  const { host, created } = createHost()
  const sessions = new TerminalSessionManager(host)
  const first = (
    await sessions.ensure({
      sessionKey: 'main',
      workspaceRoot,
      cwdPath: workspaceRoot,
      preserveFocus: true,
    })
  ).entry.terminal

  const second = (
    await sessions.ensure({
      sessionKey: 'main',
      workspaceRoot,
      cwdPath: workspaceRoot,
      preserveFocus: true,
      newTerminal: true,
    })
  ).entry.terminal

  assert.notEqual(second, first)
  assert.equal(created.length, 2)
})

test('recreates a terminal only after the previous process exited', async () => {
  const { host, created } = createHost()
  const sessions = new TerminalSessionManager(host)
  const first = (
    await sessions.ensure({
      sessionKey: 'main',
      workspaceRoot,
      cwdPath: workspaceRoot,
      preserveFocus: true,
    })
  ).entry.terminal

  first.closed = true
  const second = (
    await sessions.ensure({
      sessionKey: 'main',
      workspaceRoot,
      cwdPath: workspaceRoot,
      preserveFocus: true,
    })
  ).entry.terminal

  assert.notEqual(second, first)
  assert.equal(created.length, 2)
})

test('changes directory inside a reused PowerShell terminal', () => {
  const targetDirectory = path.join(workspaceRoot, "learner's work")
  assert.equal(
    terminalDirectoryCommand('pwsh', targetDirectory),
    `cd -- '${targetDirectory.replaceAll("'", "''")}'`,
  )
})

test('changes directory with the native syntax for cmd', () => {
  const targetDirectory = path.join(workspaceRoot, 'learner work')
  assert.equal(
    terminalDirectoryCommand('cmd', targetDirectory),
    `cd /d "${targetDirectory}"`,
  )
})

test('changes directory with fish quoting', () => {
  assert.equal(
    terminalDirectoryCommand('fish', "learner's work"),
    `cd 'learner'\\''s work'`,
  )
})

test('changes directory inside a reused POSIX terminal', () => {
  assert.equal(
    terminalDirectoryCommand('bash', "learner's work"),
    `cd -- 'learner'\\''s work'`,
  )
})

test('compares Windows terminal directories case-insensitively', () => {
  const directory =
    process.platform === 'win32' ? workspaceRoot : '/test-workspace/course'
  const differentlyCased =
    process.platform === 'win32'
      ? workspaceRoot.toUpperCase().replaceAll('\\', '/')
      : directory
  assert.equal(sameTerminalDirectory(differentlyCased, directory), true)
})

test('finds managed terminals left from before an extension host reload', () => {
  const trackedTerminal = {
    id: 3,
    name: 'Renamed by the user',
    cwdPath: terminalOne,
  }
  const staleCynapseTerminal = {
    id: 1,
    name: 'Cynapse · main',
    cwdPath: terminalOne,
  }
  const userTerminal = {
    id: 2,
    name: 'PowerShell',
    cwdPath: terminalOne,
  }
  const otherWorkspaceTerminal = {
    id: 4,
    name: 'Cynapse · main',
    cwdPath: path.join(workspaceRoot, '..', 'other'),
  }

  assert.deepEqual(
    terminalsToDisposeUnderPaths(
      [trackedTerminal],
      [staleCynapseTerminal, userTerminal, otherWorkspaceTerminal],
      workspaceRoot,
      [],
    ),
    [trackedTerminal, staleCynapseTerminal],
  )
})

test('reset paths reclaim stale terminals holding a directory handle', () => {
  const trackedInside = {
    id: 1,
    name: 'Cynapse · main',
    cwdPath: terminalOne,
  }
  const trackedOutside = {
    id: 2,
    name: 'Cynapse · main',
    cwdPath: terminalTwo,
  }
  const staleInside = {
    id: 3,
    name: 'Cynapse · main',
    cwdPath: path.join(terminalOne, 'nested'),
  }
  const staleOutside = {
    id: 4,
    name: 'Cynapse · main',
    cwdPath: terminalTwo,
  }
  const userInside = {
    id: 5,
    name: 'PowerShell',
    cwdPath: terminalOne,
  }

  assert.deepEqual(
    terminalsToDisposeUnderPaths(
      [trackedInside, trackedOutside],
      [staleInside, staleOutside, userInside],
      workspaceRoot,
      ['one'],
    ),
    [trackedInside, staleInside],
  )
})

test('keeps terminals in excluded descendant unit workspaces', () => {
  const parentTerminal = {
    id: 11,
    name: 'Cynapse · main',
    cwdPath: path.join(workspaceRoot, 'ch/parent'),
  }
  const childTerminal = {
    id: 12,
    name: 'Cynapse · child',
    cwdPath: path.join(workspaceRoot, 'ch/parent/child'),
  }

  assert.deepEqual(
    terminalsToDisposeUnderPaths(
      [],
      [parentTerminal, childTerminal],
      workspaceRoot,
      ['ch/parent'],
      ['ch/parent/child'],
    ),
    [parentTerminal],
  )
})

test('only reclaims Cynapse terminals created inside this workspace', () => {
  assert.equal(
    terminalBelongsToWorkspace(
      { name: 'Cynapse · main', cwdPath: path.join(workspaceRoot, 'one') },
      workspaceRoot,
    ),
    true,
  )
  assert.equal(
    terminalBelongsToWorkspace(
      {
        name: 'Cynapse · main',
        cwdPath: path.join(workspaceRoot, '..', 'other'),
      },
      workspaceRoot,
    ),
    false,
  )
  assert.equal(
    terminalBelongsToWorkspace(
      { name: 'PowerShell', cwdPath: workspaceRoot },
      workspaceRoot,
    ),
    false,
  )
  assert.equal(
    terminalBelongsToWorkspace({ name: 'Cynapse · main' }, workspaceRoot),
    false,
  )
})

test('session-scoped cleanup selects only the named sessions', () => {
  const entries: Array<[string, number]> = [
    ['main', 1],
    ['environment', 2],
    ['aux', 3],
  ]

  assert.deepEqual(filterSessionEntries(entries, ['environment']), [
    ['environment', 2],
  ])
  assert.deepEqual(filterSessionEntries(entries, ['environment', 'aux']), [
    ['environment', 2],
    ['aux', 3],
  ])
})

test('rejects a terminal directory that escapes through a symlink', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-terminal-'))
  const outside = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-outside-'))
  try {
    await fs.symlink(
      outside,
      path.join(root, 'escape'),
      process.platform === 'win32' ? 'junction' : 'dir',
    )
    await assert.rejects(
      resolveTerminalDirectory(root, 'escape'),
      (error: unknown) =>
        error instanceof ApplyError && error.code === 'path_outside_workspace',
    )
    assert.equal(
      await resolveTerminalDirectory(root, 'nested/missing'),
      path.join(await fs.realpath(root), 'nested', 'missing'),
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
    await fs.rm(outside, { recursive: true, force: true })
  }
})
