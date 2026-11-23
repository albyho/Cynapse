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

test('opens an existing file without saving it', async () => {
  const facade = new FakeFacade()

  const outcome = await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.preparedPath, 'src/main.rs')
  assert.equal(facade.saved, false)
})

test('inserts code at an existing line and saves it', async () => {
  const facade = new FakeFacade()
  facade.existingText = ''

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  await applyAction(
    insertAction({
      text: 'fn main() {\n',
      speed_ms: 1,
    }),
    actionFacades(facade),
  )

  assert.equal(facade.existingText, 'fn main() {\n')
  assert.equal(facade.saved, true)
})

test('holds the insertion target before typing', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {}\n'

  await applyOpenedInsert(
    insertAction({
      target: { line: 1, at: 3 },
      text: '// ',
      target_hold_ms: 125,
    }),
    facade,
  )

  assert.deepEqual(facade.selections, [
    {
      start: { line: 0, character: 3 },
      end: { line: 0, character: 3 },
    },
  ])
  assert.deepEqual(facade.delays, [125])
  assert.equal(facade.existingText, 'fn // main() {}\n')
  assert.equal(facade.saved, true)
})

test('inserts only the missing suffix of partially typed code', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main()'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  await applyAction(
    insertAction({
      text: 'fn main() {}\n',
      speed_ms: 1,
    }),
    actionFacades(facade),
  )

  assert.equal(facade.existingText, 'fn main() {}\n')
  assert.equal(facade.inserts.map((insert) => insert.text).join(''), ' {}\n')
})

test('uses a literal index to insert beside a repeated match', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'same\nsame\n'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  await applyAction(
    insertAction({
      target: {
        at: { text: 'same', index: 1 },
      },
      text: '// ',
    }),
    actionFacades(facade),
  )

  assert.equal(facade.existingText, 'same\n// same\n')
})

test('does not repeat code inserted before a literal anchor on retry', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {}\n'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  const action = insertAction({
    target: {
      at: { text: 'fn main' },
    },
    text: '// ',
  })
  await applyAction(action, actionFacades(facade))
  await applyAction(action, actionFacades(facade))

  assert.equal(facade.existingText, '// fn main() {}\n')
})

test('inserts beside the first repeated literal when index is omitted', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'same\nsame\n'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  await applyAction(
    insertAction({
      target: {
        at: { text: 'same' },
        placement: 'after',
      },
      text: '// ',
    }),
    actionFacades(facade),
  )

  assert.equal(facade.existingText, 'same// \nsame\n')
})

test('appends code at the end of an existing file', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\n}'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  await applyAction(
    insertAction({
      target: { eof: true },
      text: '\n// end',
    }),
    actionFacades(facade),
  )

  assert.equal(facade.existingText, 'fn main() {\n}\n// end')
})

test('counts insertion columns by Unicode scalar value', async () => {
  const facade = new FakeFacade()
  facade.existingText = '😀name\n'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  await applyAction(
    insertAction({
      target: { line: 1, at: 1 },
      text: '//',
    }),
    actionFacades(facade),
  )

  assert.equal(facade.existingText, '😀//name\n')
})

test('uses the document newline style when inserting multi-line code', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'first\r\nlast\r\n'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  await applyAction(
    insertAction({
      target: {
        at: { text: 'last' },
      },
      text: 'second\n',
    }),
    actionFacades(facade),
  )

  assert.equal(facade.existingText, 'first\r\nsecond\r\nlast\r\n')
})

test('rejects a position beyond the target line', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'short\n'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )

  await assert.rejects(
    () =>
      applyAction(
        insertAction({
          target: { line: 1, at: 6 },
        }),
        actionFacades(facade),
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'column_out_of_range',
  )
})

test('rejects insertion into a dirty document', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {}\n'

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  facade.target.savedText = 'let x = 1;\n'
  facade.target.dirty = true

  await assert.rejects(
    () => applyAction(insertAction({}), actionFacades(facade)),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'document_dirty',
  )
})

test('restores the complete document when insertion is cancelled', async () => {
  const original = 'fn main() {\n}\n'
  const facade = new FakeFacade()
  facade.existingText = original
  const controller = new AbortController()
  const delay = facade.delay.bind(facade)
  facade.delay = async (ms) => {
    await delay(ms)
    controller.abort()
  }

  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(facade),
  )
  const outcome = await applyAction(
    insertAction({
      target: { line: 2, at: 0 },
      text: '    println!("ok");\n'.repeat(4),
      speed_ms: 1,
    }),
    actionFacades(facade),
    controller.signal,
  )

  assert.equal(outcome, 'cancelled')
  assert.equal(facade.existingText, original)
  assert.equal(facade.saved, true)
})

test('restores editing Actions cancelled while their save is completing', async () => {
  const cases = [
    {
      original: 'fn main() {}\n',
      action: insertAction({
        target: { eof: true },
        text: '\n// inserted',
      }),
    },
    {
      original: 'fn main() {\n    temporary();\n}\n',
      action: deleteAction({
        target: textRange('    temporary();\n'),
      }),
    },
    {
      original: 'fn main() {\n    let answer = 41;\n}\n',
      action: replaceAction({
        target: {
          start: { line: 2 },
          end: { line: 2, placement: 'after' },
          expected_text: '    let answer = 41;\n',
        },
        text: '    let answer = 42;\n',
      }),
    },
    {
      original: 'fn main() {\n    let answer = 41;\n}\n',
      action: replaceAction({
        target: textRange('let answer = 41;', undefined, 'let answer = 41;\n'),
        text: 'let answer = 42;\n',
        edit_mode: 'overwrite_selection',
        speed_ms: 30,
      }),
    },
    {
      original: 'fn main() {\n    let answer = 41;\n}\n',
      action: replaceAction({
        target: textRange('let answer = 41;', undefined, 'let answer = 41;\n'),
        text: 'let answer = 42;\n',
        edit_mode: 'delete_then_type',
        speed_ms: 30,
      }),
    },
  ] as const

  for (const testCase of cases) {
    const facade = new FakeFacade()
    facade.existingText = testCase.original
    const controller = new AbortController()
    let saves = 0
    facade.save = async () => {
      saves += 1
      if (saves === 1) {
        controller.abort()
      }
      facade.saved = true
    }

    await applyAction(
      { type: 'OpenFile', path: testCase.action.path },
      actionFacades(facade),
      controller.signal,
    )
    const outcome = await applyAction(
      testCase.action,
      actionFacades(facade),
      controller.signal,
    )

    assert.equal(outcome, 'cancelled')
    assert.equal(facade.existingText, testCase.original)
    assert.equal(facade.saved, true)
  }
})

test('inserts code in roughly 50ms chunks and groups undo', async () => {
  const facade = new FakeFacade()

  await applyOpenedInsert(insertAction({ text: 'abcde', speed_ms: 30 }), facade)

  assert.deepEqual(facade.inserts, [
    { position: { line: 0, character: 0 }, text: 'ab' },
    { position: { line: 0, character: 2 }, text: 'cd' },
    { position: { line: 0, character: 4 }, text: 'e' },
  ])
  assert.equal(facade.undoStops, 2)
  assert.equal(facade.saved, true)
  assert.deepEqual(facade.delays, [60, 60])
})

test('stops a running insert within 100ms and restores the document', async (t) => {
  const facade = new FakeFacade()
  facade.realDelay = true
  const controller = new AbortController()
  const running = applyOpenedInsert(
    insertAction({ text: 'a'.repeat(100), speed_ms: 30 }),
    facade,
    controller.signal,
  )

  await new Promise((resolve) => setTimeout(resolve, 20))
  const cancelledAt = performance.now()
  controller.abort()
  const outcome = await running

  assert.equal(outcome, 'cancelled')
  assert.ok(performance.now() - cancelledAt < 100)
  assert.equal(facade.existingText, '')
  t.diagnostic(
    `insert cancellation latency: ${(performance.now() - cancelledAt).toFixed(2)} ms`,
  )
})
