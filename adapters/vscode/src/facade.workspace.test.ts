import assert from 'node:assert/strict'
import Module from 'node:module'
import { test } from 'node:test'

type Loader = {
  _load: (
    request: string,
    parent: NodeModule | undefined,
    isMain: boolean,
  ) => unknown
}

const loader = Module as unknown as Loader

test('facades reject a missing workspace root without using the open folder', async () => {
  const originalLoad = loader._load
  const fakeVscode = {
    workspace: {
      workspaceFolders: [{ uri: { fsPath: 'C:\\learner-project' } }], // test-isolation-allow: fake workspace path
    },
  }
  loader._load = function (request, parent, isMain) {
    if (request === 'vscode') {
      return fakeVscode
    }
    return originalLoad.call(this, request, parent, isMain)
  }

  try {
    const terminalModule = require('./terminalFacade') as typeof import('./terminalFacade')
    const editorModule = require('./vscodeFacade') as typeof import('./vscodeFacade')

    await assert.rejects(
      () =>
        terminalModule
          .createVSCodeTerminalFacade(undefined)
          .ensure('main', '.', false),
      (error: unknown) =>
        (error as { code?: string }).code === 'workspace_root_missing',
    )
    await assert.rejects(
      () => editorModule.createVSCodeFacade(undefined).open('main.rs'),
      (error: unknown) =>
        (error as { code?: string }).code === 'workspace_root_missing',
    )
  } finally {
    loader._load = originalLoad
  }
})
