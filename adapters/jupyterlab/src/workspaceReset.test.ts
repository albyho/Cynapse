import assert from 'node:assert/strict'
import { test } from 'node:test'

import { JupyterLabFacadeError } from './actions'
import {
  createWorkspaceResetHandler,
  type JupyterLabResetFacade,
  type WorkspaceResetRequest,
} from './workspaceReset'

class FakeResetFacade implements JupyterLabResetFacade {
  readonly calls: string[] = []
  readonly ignoredCloses = new Set<string>()
  readonly kernelFailures = new Map<string, JupyterLabFacadeError>()

  constructor(private openPaths: string[]) {}

  openNotebookPaths(): readonly string[] {
    return this.openPaths
  }

  async closeNotebook(path: string): Promise<void> {
    this.calls.push(`close:${path}`)
    if (!this.ignoredCloses.has(path)) {
      this.openPaths = this.openPaths.filter((open) => open !== path)
    }
  }

  async shutdownKernel(path: string): Promise<void> {
    this.calls.push(`kernel:${path}`)
    const failure = this.kernelFailures.get(path)
    if (failure) {
      throw failure
    }
  }

  async closeTerminals(paths: string[], excludePaths: string[]): Promise<void> {
    this.calls.push(`terminals:${paths.join(',')}:${excludePaths.join(',')}`)
  }
}

function request(
  paths: string[],
  overrides: Partial<WorkspaceResetRequest> = {},
): WorkspaceResetRequest {
  return {
    paths,
    excludePaths: [],
    closeTabs: true,
    ...overrides,
  }
}

function context() {
  return { signal: new AbortController().signal }
}

test('Unit Reset closes its own Notebooks and Kernels only', async () => {
  const facade = new FakeResetFacade([
    'ch/unit/lesson.ipynb',
    'ch/unit/nested/extra.ipynb',
    'ch/unit/child/lesson.ipynb',
    'ch/other/lesson.ipynb',
    'resources/demo.ipynb',
    '.venv/lib/site-packages/demo.ipynb',
  ])
  const handler = createWorkspaceResetHandler(facade)

  const ack = await handler(
    request(['ch/unit'], { excludePaths: ['ch/unit/child'] }),
    context(),
  )

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(facade.calls, [
    'close:ch/unit/lesson.ipynb',
    'close:ch/unit/nested/extra.ipynb',
    'kernel:ch/unit/lesson.ipynb',
    'kernel:ch/unit/nested/extra.ipynb',
  ])
  assert.deepEqual(facade.openNotebookPaths(), [
    'ch/unit/child/lesson.ipynb',
    'ch/other/lesson.ipynb',
    'resources/demo.ipynb',
    '.venv/lib/site-packages/demo.ipynb',
  ])
})

test('Course Reset closes every Notebook outside the Course Environment', async () => {
  const facade = new FakeResetFacade([
    'ch/unit/lesson.ipynb',
    'ch/unit/child/lesson.ipynb',
    'resources/demo.ipynb',
    '.venv/lib/site-packages/demo.ipynb',
  ])
  const handler = createWorkspaceResetHandler(facade)

  const ack = await handler(request(['.']), context())

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(facade.calls, [
    'close:ch/unit/lesson.ipynb',
    'close:ch/unit/child/lesson.ipynb',
    'close:resources/demo.ipynb',
    'kernel:ch/unit/lesson.ipynb',
    'kernel:ch/unit/child/lesson.ipynb',
    'kernel:resources/demo.ipynb',
  ])
  assert.deepEqual(facade.openNotebookPaths(), [
    '.venv/lib/site-packages/demo.ipynb',
  ])
})

test('an incomplete Notebook close is a typed error', async () => {
  const facade = new FakeResetFacade(['ch/unit/lesson.ipynb'])
  facade.ignoredCloses.add('ch/unit/lesson.ipynb')
  const handler = createWorkspaceResetHandler(facade)

  const ack = await handler(request(['ch/unit']), context())

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'notebook_close_incomplete')
  assert.match(ack.message ?? '', /ch\/unit\/lesson\.ipynb/)
})

test('a failed Kernel shutdown is a typed error', async () => {
  const facade = new FakeResetFacade(['ch/unit/lesson.ipynb'])
  facade.kernelFailures.set(
    'ch/unit/lesson.ipynb',
    new JupyterLabFacadeError(
      'kernel_shutdown_incomplete',
      'Kernel did not shut down',
    ),
  )
  const handler = createWorkspaceResetHandler(facade)

  const ack = await handler(request(['ch/unit']), context())

  assert.deepEqual(ack, {
    status: 'error',
    code: 'kernel_shutdown_incomplete',
    message: 'Kernel did not shut down',
  })
  assert.deepEqual(facade.calls, [
    'close:ch/unit/lesson.ipynb',
    'kernel:ch/unit/lesson.ipynb',
  ])
})

test('terminal cleanup closes terminals and never Notebooks or Kernels', async () => {
  const facade = new FakeResetFacade(['ch/unit/lesson.ipynb'])
  const handler = createWorkspaceResetHandler(facade)

  const ack = await handler(
    request(['ch/unit'], { excludePaths: ['ch/unit/child'], closeTabs: false }),
    context(),
  )

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(facade.calls, ['terminals:ch/unit:ch/unit/child'])
  assert.deepEqual(facade.openNotebookPaths(), ['ch/unit/lesson.ipynb'])
})
