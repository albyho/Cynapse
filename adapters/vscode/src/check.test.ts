import assert from 'node:assert/strict'
import * as path from 'node:path'
import { test } from 'node:test'

import { parseCheckPayload } from '@cynapse/adapter-kit'

import { dirtyDocumentsUnder, resolveUnitCheckCwd } from './check'

test('parses a complete COMMAND check payload', () => {
  const check = parseCheckPayload({
    type: 'COMMAND',
    unit_path: 'ch/main',
    session_key: 'main',
    cwd: '.',
    command: 'node --test',
    accepted_exit_codes: [0],
    expected_output: ['ok'],
    timeout_ms: 120000,
    save_dirty: true,
  })

  assert.deepEqual(check, {
    type: 'COMMAND',
    unit_path: 'ch/main',
    session_key: 'main',
    cwd: '.',
    command: 'node --test',
    accepted_exit_codes: [0],
    expected_output: ['ok'],
    timeout_ms: 120000,
    save_dirty: true,
  })
})

test('applies check defaults without inventing a session or command', () => {
  const check = parseCheckPayload({
    type: 'COMMAND',
    unit_path: 'ch/main',
    session_key: 'main',
    command: 'node --test',
  })

  assert.equal(check?.cwd, '.')
  assert.deepEqual(check?.accepted_exit_codes, [0])
  assert.deepEqual(check?.expected_output, [])
  assert.equal(check?.timeout_ms, 300000)
  assert.equal(check?.save_dirty, false)
})

test('rejects an invalid check payload', () => {
  assert.equal(parseCheckPayload({ type: 'COMMAND' }), undefined)
  assert.equal(
    parseCheckPayload({
      type: 'COMMAND',
      session_key: '',
      command: 'node --test',
    }),
    undefined,
  )
  assert.equal(
    parseCheckPayload({
      type: 'COMMAND',
      session_key: 'main',
      command: '',
    }),
    undefined,
  )
  assert.equal(
    parseCheckPayload({
      type: 'OTHER',
      unit_path: 'ch/main',
      session_key: 'main',
      command: 'node --test',
    }),
    undefined,
  )
  assert.equal(
    parseCheckPayload({
      type: 'COMMAND',
      unit_path: 'ch/main',
      session_key: 'main',
      command: 'node --test',
      accepted_exit_codes: [],
    }),
    undefined,
  )
  assert.equal(
    parseCheckPayload({
      type: 'COMMAND',
      unit_path: 'ch/main',
      session_key: 'main',
      command: 'node --test',
      timeout_ms: 0,
    }),
    undefined,
  )
  assert.equal(
    parseCheckPayload({
      type: 'COMMAND',
      unit_path: 'ch/main',
      session_key: 'main',
      command: 'node --test',
      save_dirty: 'yes',
    }),
    undefined,
  )
})

test('finds dirty file documents inside the workspace only', () => {
  const root = path.resolve('test-workspace')
  const inside = path.join(root, 'src', 'add.js')
  const outside = path.resolve('other-workspace', 'src', 'add.js')
  const documents = [
    { isDirty: true, uri: { scheme: 'file', fsPath: inside } },
    { isDirty: false, uri: { scheme: 'file', fsPath: inside } },
    { isDirty: true, uri: { scheme: 'file', fsPath: outside } },
    { isDirty: true, uri: { scheme: 'untitled', fsPath: inside } },
  ]

  assert.deepEqual(dirtyDocumentsUnder(root, documents), [documents[0]])
  assert.deepEqual(dirtyDocumentsUnder(undefined, documents), [])
})

test('keeps a challenge cwd inside its unit workspace', () => {
  const unitRoot = path.resolve('test-workspace', 'ch', 'unit')
  const outside = path.resolve('other-workspace', 'src')

  assert.equal(resolveUnitCheckCwd(unitRoot, '.'), unitRoot)
  assert.equal(resolveUnitCheckCwd(unitRoot, 'src'), path.join(unitRoot, 'src'))
  assert.equal(resolveUnitCheckCwd(unitRoot, '..'), undefined)
  assert.equal(resolveUnitCheckCwd(unitRoot, outside), undefined)
})

test('uses platform case rules for workspace containment', () => {
  const root = path.resolve('test-workspace', 'ch1')
  const caseVariant = path.join(
    path.resolve('test-workspace'),
    'CH1',
    'notes.txt',
  )
  const document = {
    isDirty: true,
    uri: { scheme: 'file', fsPath: caseVariant },
  }
  const expected = process.platform === 'win32' ? [document] : []

  assert.deepEqual(dirtyDocumentsUnder(root, [document]), expected)
})
