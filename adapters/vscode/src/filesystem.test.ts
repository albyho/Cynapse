import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import * as fs from 'node:fs/promises'
import * as os from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'

import {
  applyFileSystemAction,
  resolveMutableUnitWorkspacePath,
  resolveTerminalWorkspacePath,
  resolveUnitWorkspacePath,
  resolveWorkspacePath,
  type FileSystemFacade,
  type TargetKind,
} from './filesystem'
import { ApplyError } from './apply'
import { resolveRealWorkspacePath } from './workspacePaths'

class FakeFileSystemFacade implements FileSystemFacade {
  readonly entries = new Map<string, Exclude<TargetKind, 'missing'>>()
  readonly calls: string[] = []

  async inspect(path: string): Promise<TargetKind> {
    return this.entries.get(path) ?? 'missing'
  }

  async createDirectory(path: string, _createParents: boolean): Promise<void> {
    this.entries.set(path, 'directory')
    this.calls.push(`createDirectory:${path}`)
  }

  async createFile(path: string, _createParents: boolean): Promise<void> {
    this.entries.set(path, 'file')
    this.calls.push(`createFile:${path}`)
  }

  async deleteFile(path: string): Promise<void> {
    this.entries.delete(path)
    this.calls.push(`deleteFile:${path}`)
  }

  async directoryIsEmpty(path: string): Promise<boolean> {
    const prefix = `${path}/`
    return ![...this.entries.keys()].some((entry) => entry.startsWith(prefix))
  }

  async deleteDirectory(path: string, _recursive: boolean): Promise<void> {
    this.entries.delete(path)
    this.calls.push(`deleteDirectory:${path}`)
  }

  async confirmRecursiveDelete(_path: string): Promise<boolean> {
    return true
  }
}

test('creates a missing directory', async () => {
  const facade = new FakeFileSystemFacade()

  const outcome = await applyFileSystemAction(
    {
      type: 'CreateDirectory',
      path: 'lesson',
      create_parents: true,
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.entries.get('lesson'), 'directory')
})

test('creates a missing file and lets the facade create its parents', async () => {
  const facade = new FakeFileSystemFacade()

  const outcome = await applyFileSystemAction(
    {
      type: 'CreateFile',
      path: 'lesson/src/hello.txt',
      create_parents: true,
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.entries.get('lesson/src/hello.txt'), 'file')
})

test('treats an existing file as an idempotent create', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson/src/hello.txt', 'file')

  const outcome = await applyFileSystemAction(
    {
      type: 'CreateFile',
      path: 'lesson/src/hello.txt',
      create_parents: true,
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.deepEqual(facade.calls, [])
})

test('deletes an existing file', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson/hello.txt', 'file')

  const outcome = await applyFileSystemAction(
    {
      type: 'DeleteFile',
      path: 'lesson/hello.txt',
      if_missing: 'ignore',
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.entries.has('lesson/hello.txt'), false)
})

test('deletes an empty directory without recursion', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson', 'directory')

  const outcome = await applyFileSystemAction(
    {
      type: 'DeleteDirectory',
      path: 'lesson',
      recursive: false,
      if_missing: 'ignore',
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.entries.has('lesson'), false)
})

test('refuses to delete a non-empty directory without recursion', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson', 'directory')
  facade.entries.set('lesson/hello.txt', 'file')

  await assert.rejects(
    () =>
      applyFileSystemAction(
        {
          type: 'DeleteDirectory',
          path: 'lesson',
          recursive: false,
        },
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'directory_not_empty',
  )

  assert.deepEqual(facade.calls, [])
})

test('requires confirmation before recursively deleting a directory', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson', 'directory')
  facade.entries.set('lesson/hello.txt', 'file')
  facade.confirmRecursiveDelete = async (path) => {
    facade.calls.push(`confirmRecursiveDelete:${path}`)
    return true
  }

  const outcome = await applyFileSystemAction(
    {
      type: 'DeleteDirectory',
      path: 'lesson',
      recursive: true,
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.deepEqual(facade.calls, [
    'confirmRecursiveDelete:lesson',
    'deleteDirectory:lesson',
  ])
})

test('rejects a path outside the workspace', () => {
  const workspace = path.resolve('test-workspace')
  assert.throws(
    () => resolveWorkspacePath(workspace, path.join('..', 'outside.txt')),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'path_outside_workspace',
  )
})

test('allows a write inside the unit workspace', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-unit-'))
  try {
    await fs.mkdir(path.join(root, 'ch/intro'), { recursive: true })
    assert.equal(
      await resolveUnitWorkspacePath(root, 'ch/intro', 'ch/intro/main.rs'),
      path.join(root, 'ch/intro/main.rs'),
    )
    assert.equal(
      await resolveUnitWorkspacePath(root, 'ch/intro', 'ch/intro/new/main.rs'),
      path.join(root, 'ch/intro/new/main.rs'),
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('rejects a filesystem action at the unit workspace root', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-unit-'))
  try {
    await fs.mkdir(path.join(root, 'ch/intro'), { recursive: true })
    await assert.rejects(
      resolveMutableUnitWorkspacePath(root, 'ch/intro', 'ch/intro'),
      (error: unknown) =>
        error instanceof ApplyError &&
        error.code === 'workspace_root_not_mutable',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('allows a terminal cwd at the course workspace root', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-unit-'))
  try {
    await fs.mkdir(path.join(root, 'ch/intro'), { recursive: true })
    assert.equal(
      await resolveTerminalWorkspacePath(root, 'ch/intro', '.'),
      path.resolve(root),
    )
    assert.equal(
      await resolveTerminalWorkspacePath(root, 'ch/intro', 'ch/intro'),
      path.join(root, 'ch/intro'),
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('rejects a terminal cwd outside the unit workspace', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-unit-'))
  try {
    await fs.mkdir(path.join(root, 'ch/intro'), { recursive: true })
    await fs.mkdir(path.join(root, 'ch/other'), { recursive: true })
    await assert.rejects(
      resolveTerminalWorkspacePath(root, 'ch/intro', 'ch/other'),
      (error: unknown) =>
        error instanceof ApplyError && error.code === 'path_outside_unit',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('rejects a write outside the unit workspace', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-unit-'))
  try {
    await fs.mkdir(path.join(root, 'ch/intro'), { recursive: true })
    await assert.rejects(
      resolveUnitWorkspacePath(root, 'ch/intro', 'ch/advanced/main.rs'),
      (error: unknown) =>
        error instanceof ApplyError && error.code === 'path_outside_unit',
    )
    await assert.rejects(
      resolveUnitWorkspacePath(root, '../outside', 'ch/intro/main.rs'),
      (error: unknown) =>
        error instanceof ApplyError && error.code === 'invalid_unit_path',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('rejects a unit path that escapes through a junction', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-unit-'))
  try {
    await fs.mkdir(path.join(root, 'ch/intro'), { recursive: true })
    await fs.mkdir(path.join(root, 'ch/advanced'), { recursive: true })
    await fs.symlink(
      path.join(root, 'ch/advanced'),
      path.join(root, 'ch/intro/link'),
      process.platform === 'win32' ? 'junction' : 'dir',
    )
    await assert.rejects(
      resolveUnitWorkspacePath(root, 'ch/intro', 'ch/intro/link/main.rs'),
      (error: unknown) =>
        error instanceof ApplyError && error.code === 'path_outside_unit',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('rejects a file create when the target is a directory', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson', 'directory')

  await assert.rejects(
    () =>
      applyFileSystemAction(
        {
          type: 'CreateFile',
          path: 'lesson',
        },
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'target_type_conflict',
  )
})

test('rejects creation when parents are disabled and missing', async () => {
  const facade = new FakeFileSystemFacade()

  await assert.rejects(
    () =>
      applyFileSystemAction(
        {
          type: 'CreateDirectory',
          path: 'lesson/nested',
          create_parents: false,
        },
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'parent_missing',
  )
})

test('errors when a deleted target is missing by default', async () => {
  const facade = new FakeFileSystemFacade()

  await assert.rejects(
    () =>
      applyFileSystemAction(
        {
          type: 'DeleteFile',
          path: 'missing.txt',
        },
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'target_missing',
  )
})

test('ignores a missing delete target when explicitly requested', async () => {
  const facade = new FakeFileSystemFacade()

  const outcome = await applyFileSystemAction(
    {
      type: 'DeleteFile',
      path: 'missing.txt',
      if_missing: 'ignore',
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.deepEqual(facade.calls, [])
})

test('keeps a recursive directory when confirmation is declined', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson', 'directory')
  facade.entries.set('lesson/hello.txt', 'file')
  facade.confirmRecursiveDelete = async () => false

  const outcome = await applyFileSystemAction(
    {
      type: 'DeleteDirectory',
      path: 'lesson',
      recursive: true,
    },
    facade,
  )

  assert.equal(outcome, 'cancelled')
  assert.equal(facade.entries.get('lesson'), 'directory')
})

test('cancels before mutating when already cancelled', async () => {
  const facade = new FakeFileSystemFacade()
  const controller = new AbortController()
  controller.abort()

  const outcome = await applyFileSystemAction(
    {
      type: 'CreateDirectory',
      path: 'lesson',
    },
    facade,
    controller.signal,
  )

  assert.equal(outcome, 'cancelled')
  assert.deepEqual(facade.calls, [])
})

test('reports cancellation when abort arrives during an atomic create', async () => {
  const facade = new FakeFileSystemFacade()
  const controller = new AbortController()
  facade.createDirectory = async (path) => {
    facade.entries.set(path, 'directory')
    controller.abort()
  }

  const outcome = await applyFileSystemAction(
    {
      type: 'CreateDirectory',
      path: 'lesson',
    },
    facade,
    controller.signal,
  )

  assert.equal(outcome, 'cancelled')
  assert.equal(facade.entries.get('lesson'), 'directory')
})

test('refuses to target the workspace root', () => {
  const workspace = path.resolve('test-workspace')
  assert.throws(
    () => resolveWorkspacePath(workspace, '.'),
    (error: unknown) =>
      error instanceof ApplyError &&
      error.code === 'workspace_root_not_mutable',
  )
})

test('rejects a directory create when the target is a file', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson', 'file')

  await assert.rejects(
    () =>
      applyFileSystemAction(
        {
          type: 'CreateDirectory',
          path: 'lesson',
        },
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'target_type_conflict',
  )
})

test('rejects a file delete when the target is a directory', async () => {
  const facade = new FakeFileSystemFacade()
  facade.entries.set('lesson', 'directory')

  await assert.rejects(
    () =>
      applyFileSystemAction(
        {
          type: 'DeleteFile',
          path: 'lesson',
        },
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'target_type_conflict',
  )
})

test('rejects a workspace path that escapes through a junction', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-filesystem-'))
  const outside = await fs.mkdtemp(path.join(os.tmpdir(), 'cynapse-outside-'))
  try {
    await fs.symlink(
      outside,
      path.join(root, 'escape'),
      process.platform === 'win32' ? 'junction' : 'dir',
    )
    await assert.rejects(
      resolveRealWorkspacePath(root, path.join(root, 'escape', 'file.txt')),
      (error: unknown) =>
        error instanceof ApplyError && error.code === 'path_outside_workspace',
    )
    assert.equal(
      await resolveRealWorkspacePath(
        root,
        path.join(root, 'nested', 'file.txt'),
      ),
      path.join(await fs.realpath(root), 'nested', 'file.txt'),
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
    await fs.rm(outside, { recursive: true, force: true })
  }
})
interface SharedFileActionCase {
  name: string
  action: Parameters<typeof applyFileSystemAction>[0]
  fs?: Record<string, 'file' | 'directory'>
  expected: { status: 'ok' | 'error'; code?: string }
}

test('matches the shared file action contract', async () => {
  const fixture = JSON.parse(
    readFileSync(
      path.resolve(
        __dirname,
        '../../../protocol/contracts/file-action-cases.json',
      ),
      'utf8',
    ),
  ) as { cases: SharedFileActionCase[] }

  for (const testCase of fixture.cases) {
    const facade = new FakeFileSystemFacade()
    for (const [entry, kind] of Object.entries(testCase.fs ?? {})) {
      facade.entries.set(entry, kind)
    }
    if (testCase.expected.status === 'ok') {
      assert.equal(
        await applyFileSystemAction(testCase.action, facade),
        'done',
        testCase.name,
      )
    } else {
      await assert.rejects(
        () => applyFileSystemAction(testCase.action, facade),
        (error: unknown) =>
          error instanceof ApplyError && error.code === testCase.expected.code,
        testCase.name,
      )
    }
  }
})
