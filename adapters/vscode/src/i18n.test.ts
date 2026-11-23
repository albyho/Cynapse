import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

import { t } from './i18n'

const root = path.join(__dirname, '..')
const english = readBundle('bundle.l10n.json')
const chinese = readBundle('bundle.l10n.zh-cn.json')
const englishPackage = readBundle('package.nls.json')
const chinesePackage = readBundle('package.nls.zh-cn.json')

function readBundle(name: string): Record<string, string> {
  const file = name.startsWith('package.')
    ? path.join(root, name)
    : path.join(root, 'l10n', name)
  return JSON.parse(readFileSync(file, 'utf8')) as Record<string, string>
}

test('the English fallback fills placeholders without VS Code', () => {
  assert.equal(t('Hello {0}', 'world'), 'Hello world')
  assert.equal(t('No placeholders'), 'No placeholders')
})

test('English and Chinese bundles contain the same keys', () => {
  assert.deepEqual(Object.keys(english).sort(), Object.keys(chinese).sort())
  assert.deepEqual(
    Object.keys(englishPackage).sort(),
    Object.keys(chinesePackage).sort(),
  )
})

test('every translated source string has a bundle entry', () => {
  const messages = new Set<string>()
  for (const file of readdirSync(path.join(root, 'src'))) {
    if (!file.endsWith('.ts') || file.endsWith('.test.ts')) {
      continue
    }
    const source = readFileSync(path.join(root, 'src', file), 'utf8')
    for (const match of source.matchAll(/\bt\(\s*'((?:[^'\\]|\\.)*)'/g)) {
      messages.add(
        match[1].replaceAll("\\'", "'").replaceAll('\\"', '"'),
      )
    }
  }
  assert.deepEqual(
    [...messages].sort(),
    Object.keys(english).sort(),
    'bundle.l10n.json must list exactly the strings passed to t()',
  )
})
