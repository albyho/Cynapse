import assert from 'node:assert/strict'
import Module from 'node:module'
import * as path from 'node:path'
import { test } from 'node:test'

type Loader = {
  _load: (
    request: string,
    parent: NodeModule | undefined,
    isMain: boolean,
  ) => unknown
}

const loader = Module as unknown as Loader

test('workspace reset fails when a managed terminal does not close', async () => {
  const workspaceRoot = path.resolve('terminal-facade-test-workspace')
  const terminal = {
    name: 'Cynapse · main',
    creationOptions: {
      cwd: path.join(workspaceRoot, 'unit'),
    },
    exitStatus: undefined,
    dispose(): void {},
  }
  const fakeVscode = {
    FileType: {
      File: 1,
      Directory: 2,
      SymbolicLink: 64,
    },
    FileSystemError: class extends Error {
      code = 'FileNotFound'
    },
    Uri: {
      file: (value: string) => ({ fsPath: value }),
    },
    workspace: {
      fs: {
        stat: async () => ({ type: 2 }),
      },
    },
    window: {
      terminals: [terminal],
      onDidCloseTerminal: () => ({ dispose(): void {} }),
    },
  }
  const originalLoad = loader._load
  loader._load = function (request, parent, isMain) {
    if (request === 'vscode') {
      return fakeVscode
    }
    return originalLoad.call(this, request, parent, isMain)
  }

  try {
    const terminalModule =
      require('./terminalFacade') as typeof import('./terminalFacade')
    const facade = terminalModule.createVSCodeTerminalFacade(workspaceRoot)

    await assert.rejects(
      () => facade.disposeUnderPaths([], []),
      (error: unknown) =>
        error instanceof Error &&
        error.message.includes('did not close before workspace reset'),
    )
  } finally {
    loader._load = originalLoad
  }
})

test('rejects a directory completion marker', async () => {
  const workspaceRoot = path.resolve('terminal-facade-test-workspace')
  const terminalModule =
    require('./terminalFacade') as typeof import('./terminalFacade')
  const facade = terminalModule.createVSCodeTerminalFacade(workspaceRoot)

  await assert.rejects(
    () => facade.completionMarkerExists('src'),
    (error: unknown) =>
      error instanceof Error && error.message.includes('is not a file'),
  )
})
