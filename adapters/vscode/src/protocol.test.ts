import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

import Ajv2020 from 'ajv/dist/2020'
import addFormats from 'ajv-formats'

import { parseCourseAction } from './actions'

const protocolRoot = path.resolve(__dirname, '../../../protocol')
const projectRoot = path.resolve(__dirname, '../../..')
const schema = JSON.parse(
  readFileSync(path.join(protocolRoot, 'schema.json'), 'utf8'),
) as object
const fixtureFiles = collectJsonFiles(
  path.join(protocolRoot, 'test-data'),
).sort()
const invalidFixtureFiles = collectJsonFiles(
  path.join(protocolRoot, 'invalid-test-data'),
).sort()

function collectJsonFiles(directory: string): string[] {
  if (!existsSync(directory)) {
    return []
  }
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      return collectJsonFiles(entryPath)
    }
    return entry.name.endsWith('.json') ? [entryPath] : []
  })
}

test('all protocol golden fixtures match the shared schema', () => {
  const ajv = new Ajv2020({
    allErrors: true,
    strict: false,
  })
  addFormats(ajv)
  const validate = ajv.compile(schema)

  for (const fixtureFile of fixtureFiles) {
    const fixture = JSON.parse(readFileSync(fixtureFile, 'utf8'))
    assert.equal(
      validate(fixture),
      true,
      `${path.relative(projectRoot, fixtureFile)}: ${ajv.errorsText(validate.errors)}`,
    )
  }
})

test('representative invalid protocol fixtures are rejected by the shared schema', () => {
  const ajv = new Ajv2020({
    allErrors: true,
    strict: false,
  })
  addFormats(ajv)
  const validate = ajv.compile(schema)

  assert.ok(invalidFixtureFiles.length >= 5)
  for (const fixtureFile of invalidFixtureFiles) {
    const fixture = JSON.parse(readFileSync(fixtureFile, 'utf8'))
    assert.equal(
      validate(fixture),
      false,
      path.relative(projectRoot, fixtureFile),
    )
  }
})

test('shared schema rejects invalid code-editing combinations', () => {
  const ajv = new Ajv2020({
    allErrors: true,
    strict: false,
  })
  addFormats(ajv)
  const validate = ajv.compile(schema)
  const invalidPayloads = [
    {
      type: 'InsertText',
      path: 'src/main.rs',
      target: { kind: 'line', line: 1 },
      text: '',
    },
    {
      type: 'InsertText',
      path: 'src/main.rs',
      target: { kind: 'literal', text: 'fn main' },
      text: '// ',
    },
    {
      type: 'InsertText',
      path: 'src/main.rs',
      target: { kind: 'line', line: 1 },
      text: 'fn main() {}\n',
      target_hold_ms: 0,
    },
    {
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        cursor: {
          line: 1,
          column: 0,
          count: 0,
        },
      },
      direction: 'backspace',
    },
    {
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        start: { text: 'temporary' },
        end: { text: 'temporary' },
        expected_text: 'temporary',
      },
      target_hold_ms: 0,
    },
    {
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: {
        start: { line: 1 },
        end: { line: 1 },
        expected_text: 'old',
      },
      text: '',
    },
    {
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: {
        start: { text: '// start' },
        end: { text: '// end' },
      },
      text: 'new',
    },
    {
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: {
        start: { text: 'old' },
        end: { text: 'old' },
        expected_text: 'old',
      },
      text: 'new',
      target_hold_ms: 0,
    },
    {
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: {
        start: { text: 'old' },
        end: { text: 'old' },
        expected_text: 'old',
      },
      text: 'new',
      selection_hold_ms: 200,
    },
    {
      type: 'ReplaceText',
      target_path: 'src/main.rs',
      target: { kind: 'literal', text: 'old' },
      text: 'new',
    },
    {
      type: 'ReplaceText',
      path: 'src/main.rs',
      selector: { kind: 'literal', text: 'old' },
      replacement: 'new',
    },
  ]

  for (const payload of invalidPayloads) {
    assert.equal(
      validate({
        v: 1,
        id: '00000000-0000-4000-8000-000000000000',
        type: 'action',
        payload,
      }),
      false,
    )
  }
})

test('legacy code action names are rejected by the shared schema', () => {
  const ajv = new Ajv2020({ allErrors: true, strict: false })
  addFormats(ajv)
  const validate = ajv.compile(schema)
  const legacyPayloads = [
    {
      type: 'INSERT_CODE',
      path: 'src/main.rs',
      target: { eof: true },
      code: 'fn main() {}\n',
    },
    {
      type: 'REPLACE_CODE',
      path: 'src/main.rs',
      target: {
        start: { text: 'let x = 5;' },
        end: { text: 'let x = 5;' },
        expected_text: 'let x = 5;',
      },
      code: 'let mut x = 5;',
    },
    {
      type: 'DELETE_CODE',
      path: 'src/main.rs',
      target: { cursor: { line: 1, column: 0, count: 1 } },
      direction: 'backspace',
    },
    {
      type: 'REVEAL_CODE',
      path: 'README.md',
      start: { line: 1 },
    },
  ]

  for (const payload of legacyPayloads) {
    assert.equal(
      validate({
        v: 1,
        id: '00000000-0000-4000-8000-000000000000',
        type: 'action',
        payload,
      }),
      false,
    )
  }
})

const JUPYTERLAB_ONLY_ACTIONS = new Set([
  'JupyterLabOpenNotebook',
  'JupyterLabCellSelect',
  'JupyterLabCellRun',
  'JupyterLabRestartKernel',
  'JupyterLabInterruptKernel',
  'JupyterLabOpenPreview',
  'JupyterLabCloseDocument',
  'JupyterLabCellInsertText',
  'JupyterLabCellInsert',
  'JupyterLabCellMove',
  'JupyterLabCellDuplicate',
  'JupyterLabCellDelete',
])

test('shared Protocol test data cover every supported Action', () => {
  const actionFixtures = fixtureFiles.filter((fixtureFile) =>
    path.basename(fixtureFile).startsWith('action_'),
  )
  const actionTypes = new Set<string>()

  for (const fixtureFile of actionFixtures) {
    const fixture = JSON.parse(readFileSync(fixtureFile, 'utf8')) as {
      payload: { type: string }
    }
    if (JUPYTERLAB_ONLY_ACTIONS.has(fixture.payload.type)) {
      assert.equal(
        parseCourseAction(fixture.payload),
        undefined,
        `the VS Code Adapter must reject JupyterLab-only ${
          fixture.payload.type
        } (${path.relative(projectRoot, fixtureFile)})`,
      )
    } else {
      assert.ok(
        parseCourseAction(fixture.payload),
        path.relative(projectRoot, fixtureFile),
      )
    }
    actionTypes.add(fixture.payload.type)
  }

  assert.deepEqual([...actionTypes].sort(), [
    'ClearReveal',
    'CloseFile',
    'ClosePreview',
    'CreateDirectory',
    'CreateFile',
    'DeleteDirectory',
    'DeleteFile',
    'DeleteText',
    'InsertText',
    'JupyterLabCellDelete',
    'JupyterLabCellDuplicate',
    'JupyterLabCellInsert',
    'JupyterLabCellInsertText',
    'JupyterLabCellMove',
    'JupyterLabCellRun',
    'JupyterLabCellSelect',
    'JupyterLabCloseDocument',
    'JupyterLabInterruptKernel',
    'JupyterLabOpenNotebook',
    'JupyterLabOpenPreview',
    'JupyterLabRestartKernel',
    'OpenFile',
    'OpenFileWithPreview',
    'OpenPreview',
    'Present',
    'ReplaceText',
    'RevealText',
    'TerminalCommand',
    'TerminalEnsure',
  ])
})

test('text actions normalize their optional fields', () => {
  assert.deepEqual(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: { eof: true },
      text: 'fn main() {}\n',
    }),
    {
      type: 'InsertText',
      path: 'src/main.rs',
      target: { eof: true },
      text: 'fn main() {}\n',
    },
  )

  const replaceTarget = {
    start: { at: { text: 'let x = 5;' } },
    end: { at: { text: 'let x = 5;' }, placement: 'after' },
    expected_text: 'let x = 5;\n',
  }
  assert.deepEqual(
    parseCourseAction({
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: replaceTarget,
      text: 'let mut x = 5;',
    }),
    {
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: replaceTarget,
      text: 'let mut x = 5;',
    },
  )

  assert.equal(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: { eof: true },
      text_path: 'snippet.rs',
    }),
    undefined,
    'the Adapter never receives text_path',
  )
  assert.equal(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: { eof: true },
      text: '',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        start: { at: { text: '// temporary' } },
        end: { at: { text: '// temporary' }, placement: 'after' },
        expected_text: '// temporary',
      },
    })?.type,
    'DeleteText',
  )
  assert.equal(
    parseCourseAction({
      type: 'RevealText',
      path: 'README.md',
      start: { line: 1, column: 0 },
    })?.type,
    'RevealText',
  )
})

test('legacy code action names and fields are rejected', () => {
  const replaceTarget = {
    start: { at: { text: 'let x = 5;' } },
    end: { at: { text: 'let x = 5;' }, placement: 'after' },
    expected_text: 'let x = 5;\n',
  }
  for (const payload of [
    {
      type: 'INSERT_CODE',
      path: 'src/main.rs',
      target: { eof: true },
      code: 'fn main() {}\n',
    },
    {
      type: 'REPLACE_CODE',
      path: 'src/main.rs',
      target: replaceTarget,
      code: 'let mut x = 5;',
    },
    {
      type: 'DELETE_CODE',
      path: 'src/main.rs',
      target: {
        start: { text: '// temporary' },
        end: { text: '// temporary' },
        expected_text: '// temporary',
      },
    },
    {
      type: 'REVEAL_CODE',
      path: 'README.md',
      start: { line: 1, column: 0 },
    },
    {
      type: 'InsertText',
      path: 'src/main.rs',
      target: { eof: true },
      code: 'fn main() {}\n',
    },
    {
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: replaceTarget,
      code: 'let mut x = 5;',
    },
  ]) {
    assert.equal(parseCourseAction(payload), undefined)
  }
})

test('the VS Code Adapter rejects actions outside the shared schema', () => {
  assert.equal(
    parseCourseAction({
      type: 'Present',
      content: 'unsupported',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'CreateFile',
      path: 'lesson/hello.txt',
      recursive: true,
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'DeleteFile',
      path: 'lesson/hello.txt',
      if_missing: 'remove-anyway',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'OpenFile',
      path: '',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: { kind: 'literal', text: 'fn main' },
      text: '// ',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: { line: 1 },
      text: '',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: { line: 1 },
      text: 'fn main() {}\n',
      effect: 'instant',
    }),
    undefined,
    'effect is no longer part of the text edit protocol',
  )
  assert.equal(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: {
        at: { text: 'fn main', index: -1 },
      },
      text: '// ',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'TYPE_CODE',
      target_path: 'src/main.rs',
      line: 1,
      speed_ms: 30,
      text: 'fn main() {}\n',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: {
        start: { text: 'let x = 5;' },
        end: { text: 'let x = 5;' },
        expected_text: 'let x = 5;',
      },
      text: 'let mut x = 5;',
      effect: 'instant',
    }),
    undefined,
    'effect is no longer part of the text edit protocol',
  )
  assert.equal(
    parseCourseAction({
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: { at: { text: 'let x = 5;' } },
      text: 'let mut x = 5;',
      edit_mode: 'delete_then_type',
    }),
    undefined,
    'edit_mode requires a positive speed',
  )
  assert.equal(
    parseCourseAction({
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        cursor: {
          line: 1,
          column: 0,
          count: 0,
        },
      },
      direction: 'backspace',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        cursor: {
          line: 1,
          column: 0,
          count: 1,
        },
      },
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        cursor: {
          line: 1,
          column: 0,
          direction: 'backspace',
          count: 1,
        },
      },
      direction: 'backspace',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'CreateDirectory',
      path: '',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: '.',
      command: '',
      accepted_exit_codes: [0],
      expected_output: [],
      timeout_ms: 300000,
    }),
    undefined,
  )
  assert.ok(
    parseCourseAction({
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: '.',
      command: 'echo test',
      accepted_exit_codes: [0],
      expected_output: [],
      timeout_ms: 300000,
      new_terminal: true,
    }),
  )
  assert.equal(
    parseCourseAction({
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: '.',
      command: 'echo test',
      accepted_exit_codes: [0],
      expected_output: [],
      timeout_ms: 300000,
      new_terminal: 'yes',
    }),
    undefined,
  )
  assert.equal(
    parseCourseAction({
      type: 'TerminalCommand',
      session_key: 'main',
      cwd: '.',
      command: 'pwd',
      accepted_exit_codes: [],
      expected_output: [],
      timeout_ms: 300000,
    }),
    undefined,
  )
})

test('accepts kindless edit ranges and cursor deletion targets', () => {
  assert.ok(
    parseCourseAction({
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: {
        start: { line: 1 },
        end: { at: { text: 'return' }, placement: 'before' },
        expected_text: 'fn main() {',
      },
      text: 'fn run() {',
      speed_ms: null,
    }),
  )
  assert.ok(
    parseCourseAction({
      type: 'InsertText',
      path: 'src/main.rs',
      target: { line: 1, at: 4 },
      text: 'let answer = 42;',
      target_hold_ms: 150,
    }),
  )
  assert.ok(
    parseCourseAction({
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        cursor: { line: 1, column: 0, count: 3 },
        expected_text: 'abc',
      },
      direction: 'delete',
      target_hold_ms: 150,
    }),
  )
  assert.ok(
    parseCourseAction({
      type: 'DeleteText',
      path: 'src/main.rs',
      target: {
        start: { at: { text: 'temporary' } },
        end: { at: { text: 'temporary' }, placement: 'after' },
        expected_text: 'temporary',
      },
      target_hold_ms: 200,
    }),
  )
  assert.ok(
    parseCourseAction({
      type: 'ReplaceText',
      path: 'src/main.rs',
      target: {
        start: { at: { text: 'old' } },
        end: { at: { text: 'old' }, placement: 'after' },
        expected_text: 'old',
      },
      text: 'new',
      speed_ms: 30,
      edit_mode: 'overwrite_selection',
      target_hold_ms: 200,
    }),
  )
})
