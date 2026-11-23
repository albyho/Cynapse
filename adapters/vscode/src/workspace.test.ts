import assert from 'node:assert/strict'
import * as path from 'node:path'
import { test } from 'node:test'

import { workspaceRootFromConfig, shouldOpenWorkspace } from './workspace'

test('requires a configured workspace root without a fallback', () => {
  assert.equal(workspaceRootFromConfig(undefined), undefined)
  assert.equal(workspaceRootFromConfig('   '), undefined)

  const root = path.resolve('course-workspace')
  assert.equal(workspaceRootFromConfig(root), root)
})

test('does not open anything when the course has no workspace', () => {
  assert.equal(shouldOpenWorkspace(undefined, undefined), false)
  assert.equal(shouldOpenWorkspace(path.resolve('existing'), '   '), false)
})

test('opens a course workspace when the current window has none', () => {
  assert.equal(
    shouldOpenWorkspace(undefined, path.resolve('test-workspace')),
    true,
  )
})

test('reuses a window already showing the course workspace', () => {
  const target = path.resolve('test-workspace')

  assert.equal(shouldOpenWorkspace(target, target), false)
})

test('uses platform case rules for workspace equality', () => {
  const target = path.resolve('test-workspace')
  const differentlyCased = target.toUpperCase()

  assert.equal(
    shouldOpenWorkspace(differentlyCased, target),
    process.platform === 'win32' ? false : true,
  )
})

test('switches a window showing another workspace', () => {
  assert.equal(
    shouldOpenWorkspace(
      path.resolve('other-workspace'),
      path.resolve('test-workspace'),
    ),
    true,
  )
})
