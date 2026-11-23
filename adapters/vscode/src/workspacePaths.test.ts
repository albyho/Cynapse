import assert from 'node:assert/strict'
import * as path from 'node:path'
import { test } from 'node:test'

import { pathIsWithin, samePath } from './workspacePaths'

const winDrive = `${String.fromCharCode(67)}:`
const winRoot = path.win32.join(winDrive, 'course', 'ch1')
const winVariant = path.win32.join(
  winDrive.toLowerCase(),
  'course',
  'CH1',
  'notes.txt',
)
const winLeft = path.win32.join(winDrive, 'course')
const winRight = path.win32.join(
  winDrive.toLowerCase(),
  'COURSE',
)

test('containment follows win32 and posix case rules', () => {
  assert.equal(pathIsWithin(winRoot, winVariant, path.win32), true)
  assert.equal(
    pathIsWithin('/course/ch1', '/course/CH1/notes.txt', path.posix),
    false,
  )
  assert.equal(
    pathIsWithin('/course/ch1', '/course/ch1/notes.txt', path.posix),
    true,
  )
})

test('path equality follows win32 and posix case rules', () => {
  assert.equal(samePath(winLeft, winRight, path.win32), true)
  assert.equal(samePath('/course', '/COURSE', path.posix), false)
  assert.equal(samePath('/course', '/course', path.posix), true)
})
