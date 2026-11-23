import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'

import {
  findDuplicateFixtureNames,
  findForbiddenReferences,
} from './componentTestIsolation.mjs'

test('rejects relative references to removable content directories', () => {
  assert.deepEqual(
    findForbiddenReferences(
      [
        "const course = path.join(root, '../../../courses/demo');",
        'const allowed = path.join(root, "test-data/demo");',
      ].join('\n'),
    ),
    [{ line: 1, reference: '../../../courses/' }],
  )
})

test('rejects direct and absolute content paths', () => {
  assert.deepEqual(
    findForbiddenReferences(
      [
        "const direct = 'courses/demo';",
        "const absolute = 'D:\\\\course\\\\workspace';",
        "const posix = '/tmp/course';",
      ].join('\n'),
    ),
    [
      { line: 1, reference: 'courses/' },
      { line: 2, reference: 'D:/' },
      { line: 3, reference: '/tmp/' },
    ],
  )
})

test('rejects bare content collection names unless explicitly allowed', () => {
  assert.deepEqual(
    findForbiddenReferences(
      [
        "const course = path.join(root, 'courses');",
        "const allowed = path.join(root, 'courses'); // test-isolation-allow: temporary root",
      ].join('\n'),
    ),
    [{ line: 1, reference: 'courses' }],
  )
})

test('allows component-owned and protocol test data', () => {
  assert.deepEqual(
    findForbiddenReferences(
      [
        "const client = path.join(root, '../test-data/demo');",
        "const shared = path.join(root, '../../course/test-data/demo');",
        "const protocol = path.join(root, '../../../protocol/test-data/hello.json');",
        'const workspace = path.resolve("test-workspace");',
      ].join('\n'),
    ),
    [],
  )
})

test('rejects fixture names owned by both course and client', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'cynapse-isolation-'))
  try {
    for (const directory of [
      'course/test-data/shared',
      'course/test-data/course-only',
      'client/test-data/shared',
      'client/test-data/client-only',
    ]) {
      await mkdir(path.join(root, directory), { recursive: true })
    }
    assert.deepEqual(await findDuplicateFixtureNames(root), ['shared'])
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('rejects course sources borrowing Client test data', () => {
  const source =
    "const client = path.join(root, '../../client/test-data/demo');"
  assert.deepEqual(findForbiddenReferences(source, 'course'), [
    { line: 1, reference: '../../client/test-data/' },
  ])
  assert.deepEqual(findForbiddenReferences(source, 'client'), [])
})
