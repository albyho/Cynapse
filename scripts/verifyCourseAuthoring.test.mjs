import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'

import { courseTestDirectories } from './verifyCourseAuthoring.mjs'

test('discovers only test directories that exist', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'cynapse-course-authoring-'))
  try {
    await mkdir(path.join(root, 'courses', 'with-tests', 'tests'), {
      recursive: true,
    })
    await mkdir(path.join(root, 'courses', 'without-tests'), {
      recursive: true,
    })
    await mkdir(path.join(root, 'courses', 'also-tests', 'tests'), {
      recursive: true,
    })

    assert.deepEqual(courseTestDirectories(path.join(root, 'courses')), [
      path.join(root, 'courses', 'also-tests', 'tests'),
      path.join(root, 'courses', 'with-tests', 'tests'),
    ])
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('returns no directories when the courses root is absent', () => {
  assert.deepEqual(
    courseTestDirectories(path.join(tmpdir(), 'cynapse-missing-courses')),
    [],
  )
})
