import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { findStaleLiterals, removedLocaleValues } from './checkI18nLiterals.mjs'

test('removedLocaleValues reads changed copy and ignores short values', () => {
  const diff = [
    'diff --git a/client/i18n/locales/common.en.yml b/client/i18n/locales/common.en.yml',
    '--- a/client/i18n/locales/common.en.yml',
    '+++ b/client/i18n/locales/common.en.yml',
    '@@ -1,2 +1,2 @@',
    '-old.key: "The old sentence is long enough to track"',
    '+new.key: "The new sentence is long enough to track"',
    '-short.key: "OK"',
  ].join('\n')

  assert.deepEqual(removedLocaleValues(diff), [
    { key: 'old.key', value: 'The old sentence is long enough to track' },
  ])
})

test('findStaleLiterals reports the file and line of a duplicated value', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'i18n-literals-'))
  try {
    const file = path.join(dir, 'language.rs')
    writeFileSync(file, 'fn test() {\n  assert_eq!("The old sentence is long enough to track");\n}\n')
    const findings = findStaleLiterals(
      [{ key: 'old.key', value: 'The old sentence is long enough to track' }],
      [file]
    )

    assert.deepEqual(findings, [
      {
        key: 'old.key',
        value: 'The old sentence is long enough to track',
        file,
        line: 2,
      },
    ])
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
