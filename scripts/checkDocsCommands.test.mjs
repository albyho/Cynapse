import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  checkDocs,
  checkScriptReference,
  extractPnpmCommands,
  extractRustVersion,
  extractScriptTable,
} from './checkDocsCommands.mjs'

function temporaryRepository() {
  return mkdtempSync(path.join(tmpdir(), 'cynapse-docs-'))
}

test('extracts package scripts and skips pnpm built-ins', () => {
  const commands = extractPnpmCommands(`
\`\`\`powershell
pnpm install
pnpm dev:client
pnpm verify:client
pnpm --filter pkg test
\`\`\`
`)

  assert.deepEqual(commands, ['dev:client', 'verify:client'])
})

test('extracts the documented Rust requirement', () => {
  assert.equal(extractRustVersion('Requires Rust 1.92+ and Python 3.'), '1.92')
  assert.equal(extractRustVersion('需要 Rust 1.92.0+ 和 Python 3。'), '1.92')
})

test('extracts script names from the reference table', () => {
  assert.deepEqual(
    extractScriptTable(
      '| `verify` | all |\n| `verify:client` | client |\n| `build` | build |\n',
    ),
    ['build', 'verify', 'verify:client'],
  )
})

test('reports missing gate scripts and stale documented scripts', () => {
  assert.deepEqual(
    checkScriptReference(
      { verify: 'x', 'verify:client': 'x', 'review:prepare': 'x' },
      '| `verify` | all |\n| `review:prepare` | review |\n| `verify:studio` | stale |\n| `build:stale` | stale |\n',
    ),
    [
      'pnpm script reference: missing pnpm verify:client',
      'pnpm script reference: pnpm build:stale is not a package script',
      'pnpm script reference: pnpm verify:studio is not a package script',
    ],
  )
})

test('accepts matching README commands and Rust versions', () => {
  const root = temporaryRepository()
  try {
    mkdirSync(path.join(root, 'client/app'), { recursive: true })
    writeFileSync(
      path.join(root, 'package.json'),
      JSON.stringify({
        scripts: { 'dev:client': 'cargo run', 'verify:client': 'cargo test' },
      }),
    )
    writeFileSync(
      path.join(root, 'README.md'),
      'Requires Rust 1.92+.\n```powershell\npnpm dev:client\n```\n',
    )
    writeFileSync(
      path.join(root, 'README.zh-CN.md'),
      '需要 Rust 1.92+。\n```powershell\npnpm verify:client\n```\n',
    )
    writeFileSync(
      path.join(root, 'client/app/Cargo.toml'),
      '[package]\nrust-version = "1.92"\n',
    )

    assert.deepEqual(checkDocs(root), [])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('reports an unknown package script and a Rust mismatch', () => {
  const root = temporaryRepository()
  try {
    mkdirSync(path.join(root, 'client/app'), { recursive: true })
    writeFileSync(path.join(root, 'package.json'), JSON.stringify({ scripts: {} }))
    writeFileSync(
      path.join(root, 'README.md'),
      'Requires Rust 1.85+.\n```powershell\npnpm missing\n```\n',
    )
    writeFileSync(path.join(root, 'README.zh-CN.md'), '需要 Rust 1.92+。\n')
    writeFileSync(
      path.join(root, 'client/app/Cargo.toml'),
      '[package]\nrust-version = "1.92"\n',
    )

    assert.deepEqual(checkDocs(root), [
      'README.md: pnpm missing is not a package script',
      'README.md: requires Rust 1.85, but Cargo requires 1.92',
    ])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
