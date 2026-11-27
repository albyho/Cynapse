import assert from 'node:assert/strict'
import test from 'node:test'

import {
  applyCloseFile,
  applyClosePreview,
  applyOpenFile,
  applyOpenFileWithPreview,
  applyRevealText,
} from './apply'
import { applyAction, parseCourseAction } from './actions'
import { actionFacades, FakeFacade } from './testSupport'

test('OpenFile closes tracked side previews before opening the source', async () => {
  const facade = new FakeFacade()

  const outcome = await applyOpenFile(
    { type: 'OpenFile', path: 'src/main.rs' },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.closedSidePreviews, 1)
  assert.deepEqual(facade.previews, [])
})

test('OpenFileWithPreview closes old side previews before opening both views', async () => {
  const facade = new FakeFacade()

  const outcome = await applyOpenFileWithPreview(
    { type: 'OpenFileWithPreview', path: 'README.md' },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.closedSidePreviews, 1)
  assert.deepEqual(facade.previews, [{ path: 'README.md', view: 'side' }])
})

test('RevealText keeps side previews for source-analysis courses', async () => {
  const facade = new FakeFacade()

  const outcome = await applyRevealText(
    {
      type: 'RevealText',
      path: 'src/main.rs',
      start: { line: 1 },
      end: { line: 2 },
    },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.closedSidePreviews, 0)
})

test('SetCursor resolves Anchors against the current editor buffer', async () => {
  const facade = new FakeFacade()
  const buffer = 'fn main() {\r\n  🙂text text\r\n}\r\n'
  facade.existingText = buffer
  facade.target.dirty = true
  const cases = [
    [
      { line: 3, placement: 'after' },
      { line: 2, character: 1 },
    ],
    [{ line: 4 }, { line: 3, character: 0 }],
    [
      { line: 2, at: 3 },
      { line: 1, character: 4 },
    ],
    [
      { line: 2, at: 2, placement: 'after' },
      { line: 1, character: 4 },
    ],
    [{ at: { text: 'text', index: 1 } }, { line: 1, character: 9 }],
    [
      { at: { text: 'text\n}' }, placement: 'after' },
      { line: 2, character: 1 },
    ],
    [{ eof: true }, { line: 3, character: 0 }],
  ] as const

  for (const [target, position] of cases) {
    const action = parseCourseAction({
      type: 'SetCursor',
      path: 'src/main.rs',
      target,
    })
    assert.equal(action?.type, 'SetCursor')
    assert.equal(await applyAction(action, actionFacades(facade)), 'done')
    assert.deepEqual(facade.selections.at(-1), {
      start: position,
      end: position,
    })
  }
  assert.equal(facade.existingText, buffer)
  assert.equal(facade.saves, 0)
  assert.equal(facade.closedSidePreviews, 0)
  assert.equal(facade.clearReveals, 0)
  assert.deepEqual(facade.reveals, [])
})

test('SetCursor rejects invalid destinations and honors cancellation', async () => {
  const facade = new FakeFacade()
  const input = { type: 'SetCursor', path: 'src/main.rs' }
  assert.equal(parseCourseAction({ ...input, target: { line: 0 } }), undefined)
  assert.equal(
    parseCourseAction({ ...input, target: { eof: true, line: 1 } }),
    undefined,
  )
  const action = parseCourseAction({ ...input, target: { line: 2 } })
  assert.equal(action?.type, 'SetCursor')
  assert.equal(
    await applyAction(action, actionFacades(facade), AbortSignal.abort()),
    'cancelled',
  )
  assert.equal(facade.preparedPath, undefined)
  await assert.rejects(applyAction(action, actionFacades(facade)), {
    code: 'line_out_of_range',
  })
  for (const [target, code] of [
    [{ line: 1, at: 1 }, 'column_out_of_range'],
    [{ at: { text: 'not present' } }, 'code_match_missing'],
  ] as const) {
    const action = parseCourseAction({ ...input, target })
    assert.equal(action?.type, 'SetCursor')
    await assert.rejects(applyAction(action, actionFacades(facade)), { code })
  }
  assert.deepEqual(facade.selections, [])
})

test('CloseFile closes only the file tab', async () => {
  const facade = new FakeFacade()

  const outcome = await applyCloseFile(
    { type: 'CloseFile', path: 'src/main.rs' },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.deepEqual(facade.closedFiles, ['src/main.rs'])
  assert.deepEqual(facade.closedPreviews, [])
  assert.equal(facade.closedSidePreviews, 0)
})

test('ClosePreview closes only the recorded preview', async () => {
  const facade = new FakeFacade()

  const outcome = await applyClosePreview(
    { type: 'ClosePreview', path: 'README.md' },
    facade,
  )

  assert.equal(outcome, 'done')
  assert.deepEqual(facade.closedPreviews, ['README.md'])
  assert.deepEqual(facade.closedFiles, [])
  assert.equal(facade.closedSidePreviews, 0)
})
