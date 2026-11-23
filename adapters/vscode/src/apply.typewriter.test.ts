import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ApplyError, type ReplaceTextAction } from './apply'
import { applyAction } from './actions'
import {
  FakeFacade,
  replaceAction,
  deleteAction,
  textRange,
  actionFacades,
  applyOpenedReplace,
  applyOpenedDelete,
  applyOpenedInsert,
  insertAction,
} from './testSupport'

test('types over the selected text for an overwrite replacement', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\n    let x = 5;\n}\n'

  await applyOpenedReplace(
    replaceAction({
      target: textRange('let x = 5;', undefined, 'let x = 5;\n'),
      text: 'let mut x = 5;\n',
      edit_mode: 'overwrite_selection',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(facade.existingText, 'fn main() {\n    let mut x = 5;\n}\n')
  assert.deepEqual(facade.selections[0], {
    start: { line: 1, character: 4 },
    end: { line: 2, character: 0 },
  })
  assert.deepEqual(facade.selections[1], {
    start: { line: 1, character: 6 },
    end: { line: 1, character: 6 },
  })
  assert.equal(facade.replacements[0]?.text, 'le')
  assert.equal(
    facade.inserts.map((insert) => insert.text).join(''),
    't mut x = 5;\n',
  )
  assert.equal(facade.saved, true)
})

test('holds the selected replacement range before typing over it', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let x = 5;\n'

  await applyOpenedReplace(
    replaceAction({
      edit_mode: 'overwrite_selection',
      speed_ms: 30,
      target_hold_ms: 250,
    }),
    facade,
  )

  assert.deepEqual(facade.selections[0], {
    start: { line: 0, character: 0 },
    end: { line: 1, character: 0 },
  })
  assert.equal(facade.delays[0], 250)
  assert.equal(facade.existingText, 'let mut x = 5;\n')
  assert.equal(facade.saved, true)
})

test('holds the selected replacement range before one-shot replacement', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let x = 5;\n'

  await applyOpenedReplace(
    replaceAction({
      text: 'let mut x = 5;\n',
      target_hold_ms: 250,
    }),
    facade,
  )

  assert.deepEqual(facade.selections, [
    {
      start: { line: 0, character: 0 },
      end: { line: 1, character: 0 },
    },
  ])
  assert.deepEqual(facade.delays, [250])
  assert.equal(facade.existingText, 'let mut x = 5;\n')
  assert.equal(facade.saved, true)
})

test('cancels during the selection hold without editing the document', async () => {
  const original = 'let x = 5;\n'
  const facade = new FakeFacade()
  facade.existingText = original
  facade.realDelay = true
  const controller = new AbortController()
  const abortTimer = setTimeout(() => controller.abort(), 5)
  const startedAt = Date.now()

  const outcome = await applyOpenedReplace(
    replaceAction({
      target_hold_ms: 1000,
    }),
    facade,
    controller.signal,
  )
  clearTimeout(abortTimer)

  assert.equal(outcome, 'cancelled')
  assert.ok(Date.now() - startedAt < 500)
  assert.equal(facade.existingText, original)
  assert.equal(facade.replacements.length, 0)
  assert.equal(facade.saved, false)
})

test('uses the declared typewriter speed for replacement', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let x = 5;\n'

  await applyOpenedReplace(
    replaceAction({
      speed_ms: 60,
    }),
    facade,
  )

  assert.equal(facade.existingText, 'let mut x = 5;\n')
  assert.ok(facade.delays.length > 0)
  assert.ok(facade.delays.every((delay) => delay === 60))
})

test('restores the original code when a typewriter replacement is cancelled', async () => {
  const original = 'fn main() {\n    let x = 5;\n}\n'
  const facade = new FakeFacade()
  facade.existingText = original
  const controller = new AbortController()
  const delay = facade.delay.bind(facade)
  facade.delay = async (ms) => {
    await delay(ms)
    controller.abort()
  }

  const outcome = await applyOpenedReplace(
    replaceAction({
      target: textRange('let x = 5;', undefined, 'let x = 5;\n'),
      text: 'let mut changed_x = 5;\n',
      edit_mode: 'overwrite_selection',
      speed_ms: 30,
    }),
    facade,
    controller.signal,
  )

  assert.equal(outcome, 'cancelled')
  assert.equal(facade.existingText, original)
  assert.equal(facade.saved, true)
})

test('restores the original selection when delete-then-type is cancelled', async () => {
  const original = 'fn main() {\n    let x = 5;\n}\n'
  const facade = new FakeFacade()
  facade.existingText = original
  const controller = new AbortController()
  const insert = facade.insert.bind(facade)
  facade.insert = async (target, position, text, options) => {
    await insert(target, position, text, options)
    controller.abort()
  }

  const outcome = await applyOpenedReplace(
    replaceAction({
      target: textRange('let x = 5;', undefined, 'let x = 5;\n'),
      text: 'let mut changed_x = 5;\n',
      edit_mode: 'delete_then_type',
      speed_ms: 30,
    }),
    facade,
    controller.signal,
  )

  assert.equal(outcome, 'cancelled')
  assert.equal(facade.existingText, original)
  assert.equal(facade.saved, true)
})
