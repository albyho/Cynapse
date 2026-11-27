import assert from 'node:assert/strict'
import test from 'node:test'

import { publishedPath } from './syncPublic.mjs'

// These assertions are the leak guard: widening the include list or narrowing
// the exclusions has to be a deliberate edit that also edits this test.
test('publishes source that the product needs to build', () => {
  for (const file of [
    'client/app/src/main.rs',
    'adapters/vscode/src/extension.ts',
    'protocol/schema.json',
    'course/src/lib.rs',
    'packaging/msix/identity.json',
    'scripts/checkMarkdown.mjs',
    'Cargo.toml',
    'package.json',
    'LICENSE',
    'CHANGELOG.md',
  ]) {
    assert.equal(publishedPath(file), true, file)
  }
})

test('keeps internal documents and assets out of the public tree', () => {
  for (const file of [
    'docs/adr/0001-stack.md',
    'docs/audits/2026-11-23-repository-audit.md',
    'spec.md',
    'GLOSSARY.md',
    'AGENTS.md',
    'skills/cynapse-release/SKILL.md',
    'courses/rust/lesson.json',
    '.githooks/pre-commit',
    '.github/workflows/release.yml',
    'README.md',
    'README.zh-CN.md',
    '.gitignore',
    'adapters/scratch/research/demo-snap-chain.js',
    'packaging/msix/store-submission.md',
  ]) {
    assert.equal(publishedPath(file), false, file)
  }
})
