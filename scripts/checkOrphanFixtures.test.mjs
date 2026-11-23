import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  findOrphanFixtures,
  listFixtureEntries,
} from './checkOrphanFixtures.mjs'

function temporaryRepository() {
  const root = mkdtempSync(path.join(tmpdir(), 'cynapse-fixtures-'))
  mkdirSync(path.join(root, 'course/test-data/used'), { recursive: true })
  mkdirSync(path.join(root, 'course/test-data/orphan'), { recursive: true })
  mkdirSync(path.join(root, 'protocol/test-data'), { recursive: true })
  mkdirSync(path.join(root, 'client/src'), { recursive: true })
  writeFileSync(path.join(root, 'course/test-data/README.md'), 'fixture table\n')
  writeFileSync(path.join(root, 'protocol/test-data/dynamic.json'), '{}\n')
  writeFileSync(path.join(root, 'client/src/test.rs'), 'use used fixture\n')
  return root
}

test('lists top-level fixture entries and skips dynamic protocol fixtures', () => {
  const root = temporaryRepository()
  try {
    assert.deepEqual(listFixtureEntries(root), [
      'course/test-data/orphan',
      'course/test-data/used',
    ])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('reports fixtures with no source or documentation reference', () => {
  const root = temporaryRepository()
  try {
    assert.deepEqual(findOrphanFixtures(root), ['course/test-data/orphan'])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
