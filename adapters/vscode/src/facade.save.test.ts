import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import Module from 'node:module'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { test } from 'node:test'

type Loader = {
  _load: (
    request: string,
    parent: NodeModule | undefined,
    isMain: boolean,
  ) => unknown
}

type UriLike = {
  fsPath: string
  path: string
  toString: () => string
  with: (change: { path?: string }) => UriLike
}

type FakeDocument = {
  uri: UriLike
  version: number
  lineCount: number
  isDirty: boolean
  isClosed: boolean
  text: string
  saves: number
  getText(): string
  save(): Promise<boolean>
}

function fileUri(fsPath: string): UriLike {
  return {
    fsPath,
    path: fsPath.replaceAll('\\', '/'),
    toString: () => pathToFileURL(fsPath).href,
    with: (change) => fileUri(change.path ?? fsPath),
  }
}

function utf16File(text: string): Buffer {
  return Buffer.concat([
    Buffer.from([0xff, 0xfe]),
    Buffer.from(text, 'utf16le'),
  ])
}

function fakeDocument(
  filePath: string,
  text: string,
  persist: (text: string) => Promise<void>,
): FakeDocument {
  return {
    uri: fileUri(filePath),
    version: 1,
    lineCount: 2,
    isDirty: false,
    isClosed: false,
    text,
    saves: 0,
    getText(): string {
      return this.text
    },
    async save(): Promise<boolean> {
      this.saves += 1
      await persist(this.text)
      this.isDirty = false
      return true
    },
  }
}

function fakeVscodeFor(
  document: FakeDocument,
  onRawWrite?: () => void,
): unknown {
  return {
    Uri: {
      file: fileUri,
      parse: (value: string) => fileUri(fileURLToPath(value)),
    },
    FileType: { File: 1, Directory: 2, SymbolicLink: 64 },
    FileSystemError: class extends Error {},
    workspace: {
      fs: {
        stat: async () => ({ type: 1 }),
        readFile: async (uri: UriLike) => readFile(uri.fsPath),
        writeFile: async () => {
          onRawWrite?.()
        },
      },
      openTextDocument: async () => document,
    },
    window: {
      showTextDocument: async () => ({
        document,
        viewColumn: 1,
      }),
    },
  }
}

const loader = Module as unknown as Loader

/**
 * vscodeFacade 在模块求值时绑定 `vscode`，所以每个用例都要清掉模块缓存，
 * 才能拿到自己那份假实现。
 */
function loadFacade(fakeVscode: unknown): typeof import('./vscodeFacade') {
  const originalLoad = loader._load
  loader._load = function (request, parent, isMain) {
    if (request === 'vscode') {
      return fakeVscode
    }
    return originalLoad.call(this, request, parent, isMain)
  }
  try {
    delete require.cache[require.resolve('./vscodeFacade')]
    return require('./vscodeFacade') as typeof import('./vscodeFacade')
  } finally {
    loader._load = originalLoad
  }
}

test('save uses the VS Code document encoding instead of UTF-8', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'cynapse-save-'))
  const filePath = path.join(root, 'unicode.txt')
  const original = 'const greeting = "你好"\r\n'
  const edited = 'const greeting = "世界"\r\n'
  await writeFile(filePath, utf16File(original))

  const document = fakeDocument(filePath, original, async (text) => {
    await writeFile(filePath, utf16File(text))
  })
  let rawWrites = 0

  try {
    const facade = loadFacade(
      fakeVscodeFor(document, () => {
        rawWrites += 1
      }),
    ).createVSCodeFacade(root)
    const target = await facade.open('unicode.txt')
    document.text = edited
    document.isDirty = true
    await facade.save(target)

    assert.equal(document.saves, 1)
    assert.equal(rawWrites, 0)
    const saved = await readFile(filePath)
    assert.deepEqual([...saved.subarray(0, 2)], [0xff, 0xfe])
    assert.equal(saved.subarray(2).toString('utf16le'), edited)
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('save refuses to overwrite a file changed outside VS Code', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'cynapse-external-'))
  const filePath = path.join(root, 'note.txt')
  const baseline = 'baseline\n'
  await writeFile(filePath, baseline)

  const document = fakeDocument(filePath, baseline, async (text) => {
    await writeFile(filePath, text)
  })

  try {
    const facade = loadFacade(fakeVscodeFor(document)).createVSCodeFacade(root)
    const target = await facade.open('note.txt')
    // 打开之后有外部工具改了磁盘，而编辑器里的文本仍是基线：此时写入会
    // 覆盖掉别人的改动。
    const external = 'changed by another tool\n'
    await writeFile(filePath, external)

    await assert.rejects(
      facade.save(target),
      (error: Error & { code?: string }) => {
        assert.equal(error.code, 'external_file_change')
        return true
      },
    )
    assert.equal(document.saves, 0, 'the document must not be saved')
    assert.equal(await readFile(filePath, 'utf8'), external)
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})
