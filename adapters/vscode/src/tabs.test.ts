import assert from 'node:assert/strict'
import * as path from 'node:path'
import { test } from 'node:test'

import {
  pruneRecordedPreviews,
  tabsForFile,
  tabsForWorkspaceReset,
  type RecordedPreview,
  type TabLike,
} from './tabs'

// 内置 Markdown 预览：TabInputWebview，viewType 为 markdown.preview，没有 uri。
const PREVIEW_INPUT = { viewType: 'markdown.preview' }

function previewTab(): TabLike {
  // 每个预览 tab 都是独立对象；同名文件靠对象引用区分，而不是标题。
  return { input: PREVIEW_INPUT }
}

function recordedPreview(
  workspace: string,
  chapter: string,
  tab: TabLike,
): RecordedPreview<TabLike> {
  return {
    tab,
    filePath: path.join(workspace, chapter, 'INTRO.md'),
  }
}

test('prunes preview records when the user closes their tab', () => {
  const live = previewTab()
  const closed = previewTab()
  const recorded: RecordedPreview<TabLike>[] = [
    { tab: live, filePath: 'live/INTRO.md' },
    { tab: closed, filePath: 'closed/INTRO.md' },
  ]

  pruneRecordedPreviews(recorded, [live])

  assert.deepEqual(recorded, [{ tab: live, filePath: 'live/INTRO.md' }])
})

test('closes tabs for a deleted file and deleted directory only', async () => {
  const workspace = path.resolve('test-workspace')
  const tabs: TabLike[] = [
    { input: { uri: { fsPath: path.join(workspace, 'src/main.rs') } } },
    { input: { uri: { fsPath: path.join(workspace, 'keep.txt') } } },
  ]
  const groups = [{ tabs }]

  const matched = tabsForWorkspaceReset(workspace, ['src'], groups)

  assert.deepEqual(
    matched.map(
      ({ tab }) => (tab.input as { uri: { fsPath: string } }).uri.fsPath,
    ),
    [path.join(workspace, 'src/main.rs')],
  )
})

test('closes existing tabs for a whole-workspace reset', () => {
  const workspace = path.resolve('test-workspace')
  const tab: TabLike = {
    input: { uri: { fsPath: path.join(workspace, 'src/main.rs') } },
  }
  const groups = [{ tabs: [tab] }]

  const matched = tabsForWorkspaceReset(workspace, ['.'], groups)

  assert.deepEqual(matched, [{ tab, group: groups[0] }])
})

test('matches every tab showing the file across groups and diff sides', () => {
  const workspace = path.resolve('test-workspace')
  const target = path.join(workspace, 'src/main.rs')
  const textTab: TabLike = { input: { uri: { fsPath: target } } }
  const diffTab: TabLike = {
    input: {
      original: { fsPath: target },
      modified: { fsPath: target },
    },
  }
  const otherTab: TabLike = {
    input: { uri: { fsPath: path.join(workspace, 'src/other.rs') } },
  }
  const groups = [{ tabs: [textTab, otherTab] }, { tabs: [diffTab] }]

  assert.deepEqual(tabsForFile(groups, target), [
    { tab: textTab, group: groups[0] },
    { tab: diffTab, group: groups[1] },
  ])
})

test('keeps tabs in excluded descendant unit workspaces', () => {
  const workspace = path.resolve('test-workspace')
  const parentTab: TabLike = {
    input: { uri: { fsPath: path.join(workspace, 'ch/parent/own.txt') } },
  }
  const childTab: TabLike = {
    input: {
      uri: { fsPath: path.join(workspace, 'ch/parent/child/child.txt') },
    },
  }
  const groups = [{ tabs: [parentTab, childTab] }]

  const matched = tabsForWorkspaceReset(
    workspace,
    ['ch/parent'],
    groups,
    [],
    ['ch/parent/child'],
  )

  assert.deepEqual(matched, [{ tab: parentTab, group: groups[0] }])
})

test('closes both sides of a diff tab for a deleted file', async () => {
  const workspace = path.resolve('test-workspace')
  const groups = [
    {
      tabs: [
        {
          input: {
            original: {
              fsPath: path.join(workspace, 'hello_cargo/Cargo.toml'),
            },
            modified: {
              fsPath: path.join(workspace, 'hello_cargo/Cargo.toml'),
            },
          },
        },
      ],
    },
  ]

  const matched = tabsForWorkspaceReset(workspace, ['hello_cargo'], groups)

  assert.equal(matched.length, 1)
})

test('closes the recorded Markdown preview inside the reset path', async () => {
  const workspace = path.resolve('test-workspace')
  const tab = previewTab()
  const groups = [{ tabs: [tab] }]

  const matched = tabsForWorkspaceReset(
    workspace,
    ['ch1_hello_world'],
    groups,
    [recordedPreview(workspace, 'ch1_hello_world', tab)],
  )

  assert.deepEqual(
    matched.map(({ tab: matchedTab }) => matchedTab),
    [tab],
  )
})

test('keeps a preview recorded for another chapter even with the same file name', async () => {
  const workspace = path.resolve('test-workspace')
  const tab = previewTab()
  const groups = [{ tabs: [tab] }]

  const matched = tabsForWorkspaceReset(
    workspace,
    ['ch1_hello_world'],
    groups,
    [recordedPreview(workspace, 'ch2_primitives', tab)],
  )

  assert.equal(matched.length, 0)
})

test('closes only the in-scope tab when two chapters preview the same INTRO.md', async () => {
  const workspace = path.resolve('test-workspace')
  const ch1Tab = previewTab()
  const ch2Tab = previewTab()
  const groups = [{ tabs: [ch1Tab, ch2Tab] }]
  const recorded = [
    recordedPreview(workspace, 'ch1_hello_world', ch1Tab),
    recordedPreview(workspace, 'ch2_primitives', ch2Tab),
  ]

  const matched = tabsForWorkspaceReset(
    workspace,
    ['ch1_hello_world'],
    groups,
    recorded,
  )

  assert.deepEqual(
    matched.map(({ tab }) => tab),
    [ch1Tab],
  )
})

test('closes all Markdown previews when records were lost after an extension reload', async () => {
  const workspace = path.resolve('test-workspace')
  const groups = [{ tabs: [previewTab(), previewTab()] }]

  const matched = tabsForWorkspaceReset(workspace, ['ch1_hello_world'], groups)

  assert.equal(matched.length, 2)
})

test('closes a Markdown custom-editor preview by its uri', async () => {
  const workspace = path.resolve('test-workspace')
  const groups = [
    {
      tabs: [
        {
          input: {
            viewType: 'vscode.markdown.preview.editor',
            uri: {
              fsPath: path.join(workspace, 'ch1_hello_world', 'INTRO.md'),
            },
          },
        },
      ],
    },
  ]

  assert.equal(
    tabsForWorkspaceReset(workspace, ['ch1_hello_world'], groups).length,
    1,
  )
})

test('ignores unrelated webview tabs', async () => {
  const workspace = path.resolve('test-workspace')
  const groups = [
    {
      tabs: [{ input: { viewType: 'some.extension.webview' } }],
    },
  ]

  assert.equal(
    tabsForWorkspaceReset(workspace, ['ch1_hello_world'], groups).length,
    0,
  )
})

test('treats the workspace root itself as a reset target', async () => {
  const workspace = path.resolve('test-workspace')
  const tab = previewTab()
  const groups = [{ tabs: [tab] }]

  const matched = tabsForWorkspaceReset(workspace, ['.'], groups, [
    recordedPreview(workspace, 'ch1_hello_world', tab),
  ])

  assert.equal(matched.length, 1)
})

test('ignores reset paths outside the workspace', async () => {
  const workspace = path.resolve('test-workspace')
  const groups = [
    {
      tabs: [
        {
          input: {
            uri: { fsPath: path.resolve(workspace, '..', 'outside.txt') },
          },
        },
      ],
    },
  ]

  assert.equal(
    tabsForWorkspaceReset(workspace, ['../outside.txt'], groups).length,
    0,
  )
})

test('keeps a text tab under an excluded descendant unit', async () => {
  const workspace = path.resolve('test-workspace')
  const descendantFile = path.join(workspace, 'ch1', 'hello', 'src', 'main.rs')
  const groups = [
    {
      tabs: [{ input: { uri: { fsPath: descendantFile } } }],
    },
  ]

  // 重置父 Unit ch1，但 ch1/hello 的工作区被保留。
  const matched = tabsForWorkspaceReset(
    workspace,
    ['ch1'],
    groups,
    [],
    ['ch1/hello'],
  )

  assert.equal(matched.length, 0)
})

test('closes a text tab in the reset scope before the files are deleted', async () => {
  const workspace = path.resolve('test-workspace')
  const parentFile = path.join(workspace, 'ch1', 'src', 'main.rs')
  const groups = [
    {
      tabs: [{ input: { uri: { fsPath: parentFile } } }],
    },
  ]

  const matched = tabsForWorkspaceReset(workspace, ['ch1'], groups)

  assert.equal(matched.length, 1)
})

test('keeps a descendant unit preview in an excluded workspace', async () => {
  const workspace = path.resolve('test-workspace')
  const tab = previewTab()
  const descendantIntro = path.join(workspace, 'ch1', 'hello', 'INTRO.md')
  const groups = [{ tabs: [tab] }]

  const matched = tabsForWorkspaceReset(
    workspace,
    ['ch1'],
    groups,
    [{ tab, filePath: descendantIntro }],
    ['ch1/hello'],
  )

  assert.equal(matched.length, 0)
})

test('closes a parent unit preview in the reset scope', async () => {
  const workspace = path.resolve('test-workspace')
  const tab = previewTab()
  const parentIntro = path.join(workspace, 'ch1', 'INTRO.md')
  const groups = [{ tabs: [tab] }]

  const matched = tabsForWorkspaceReset(workspace, ['ch1'], groups, [
    { tab, filePath: parentIntro },
  ])

  assert.equal(matched.length, 1)
})

test('closes a tab for a file whose name starts with two dots', () => {
  const workspace = path.resolve('test-workspace')
  const groups = [
    {
      tabs: [
        {
          input: {
            uri: { fsPath: path.join(workspace, 'src', '..notes.md') },
          },
        },
      ],
    },
  ]

  const matched = tabsForWorkspaceReset(workspace, ['src'], groups)

  assert.equal(matched.length, 1)
})
