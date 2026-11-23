import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { findOrphanScripts, listScripts } from './checkOrphanScripts.mjs'

function temporaryRepository() {
  const root = mkdtempSync(path.join(tmpdir(), 'cynapse-scripts-'))
  mkdirSync(path.join(root, 'scripts'))
  mkdirSync(path.join(root, '.github/workflows'), { recursive: true })
  writeFileSync(
    path.join(root, 'package.json'),
    JSON.stringify({
      scripts: {
        used: 'node scripts/used.mjs',
        'test:used': 'node --test scripts/used.test.mjs',
      },
    }),
  )
  writeFileSync(
    path.join(root, '.github/workflows/verify.yml'),
    'run: node scripts/used.mjs\n',
  )
  writeFileSync(path.join(root, 'scripts/used.mjs'), 'export {}\n')
  writeFileSync(path.join(root, 'scripts/used.test.mjs'), 'test\n')
  writeFileSync(path.join(root, 'scripts/orphan.mjs'), 'export {}\n')
  return root
}

test('lists repo scripts', () => {
  const root = temporaryRepository()
  try {
    assert.deepEqual(listScripts(root), [
      'orphan.mjs',
      'used.mjs',
      'used.test.mjs',
    ])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('reports scripts with no package, workflow, or script reference', () => {
  const root = temporaryRepository()
  try {
    assert.deepEqual(findOrphanScripts(root), ['orphan.mjs'])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
