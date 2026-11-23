import assert from 'node:assert/strict'
import test from 'node:test'

import { markdownPaths, parseArgs } from './checkMarkdown.mjs'

test('keeps only Markdown paths', () => {
  assert.deepEqual(markdownPaths(['a.md', 'b.json', 'docs/c.MD', 'd.mdx']), [
    'a.md',
    'docs/c.MD',
  ])
})

test('defaults to all files and accepts explicit modes', () => {
  assert.deepEqual(parseArgs([]), { mode: 'all', paths: [], help: false })
  assert.equal(parseArgs(['--staged']).mode, 'staged')
  assert.equal(parseArgs(['--all']).mode, 'all')
  assert.deepEqual(parseArgs(['docs/README.md', '--help']), {
    mode: 'all',
    paths: ['docs/README.md'],
    help: true,
  })
})
