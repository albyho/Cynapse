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

test('deletes a selected literal range in reverse chunks', async () => {
  const facade = new FakeFacade()
  facade.existingText =
    'fn main() {\n    let temporary = 5;\n    println!("ok");\n}\n'
  const deletion = deleteAction({
    target: textRange(
      '    let temporary = 5;\n    println!("ok");',
      undefined,
      '    let temporary = 5;\n    println!("ok");\n',
    ),
    speed_ms: 30,
  })

  await applyOpenedDelete(deletion, facade)

  assert.equal(facade.existingText, 'fn main() {\n}\n')
  assert.ok(facade.replacements.length > 1)
  assert.ok(facade.replacements.every((edit) => edit.text === ''))
  assert.ok(facade.delays.every((delay) => delay > 0))
  assert.equal(facade.inserts.length, 0)
  assert.equal(facade.saved, true)
})

test('holds the selected delete range before one-shot deletion', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'ABCD'

  await applyOpenedDelete(
    deleteAction({
      target: textRange('BC'),
      target_hold_ms: 200,
    }),
    facade,
  )

  assert.deepEqual(facade.selections, [
    {
      start: { line: 0, character: 1 },
      end: { line: 0, character: 3 },
    },
  ])
  assert.deepEqual(facade.delays, [200])
  assert.equal(facade.existingText, 'AD')
  assert.equal(facade.saved, true)
})

test('holds the deletion cursor before one-shot deletion', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'ABCD'

  await applyOpenedDelete(
    deleteAction({
      target: {
        cursor: {
          line: 1,
          column: 3,
          count: 1,
        },
        expected_text: 'C',
      },
      direction: 'backspace',
      target_hold_ms: 175,
    }),
    facade,
  )

  assert.deepEqual(facade.selections, [
    {
      start: { line: 0, character: 3 },
      end: { line: 0, character: 3 },
    },
  ])
  assert.deepEqual(facade.delays, [175])
  assert.equal(facade.existingText, 'ABD')
  assert.equal(facade.saved, true)
})

test('deletes a range in the forward direction when requested', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'ABCD'

  await applyOpenedDelete(
    deleteAction({
      target: textRange('BC'),
      direction: 'delete',
      speed_ms: 51,
    }),
    facade,
  )

  assert.equal(facade.existingText, 'AD')
  assert.deepEqual(
    facade.replacements.map((edit) => [
      edit.start.character,
      edit.end.character,
    ]),
    [
      [1, 2],
      [1, 2],
    ],
  )
})

test('deletes the selected text before typing a replace-mode edit', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\r\n    let answer = 41;\r\n}\r\n'

  await applyOpenedReplace(
    replaceAction({
      target: textRange('let answer = 41;', undefined, 'let answer = 41;\n'),
      text: 'let answer = 42;\n',
      edit_mode: 'delete_then_type',
      speed_ms: 60,
    }),
    facade,
  )

  assert.equal(
    facade.existingText,
    'fn main() {\r\n    let answer = 42;\r\n}\r\n',
  )
  assert.deepEqual(facade.replacements[0]?.text, '')
  assert.deepEqual(facade.selections[1], {
    start: { line: 1, character: 20 },
    end: { line: 1, character: 20 },
  })
  assert.deepEqual(
    facade.inserts.map((insert) => insert.text).join(''),
    'let answer = 42;\r\n',
  )
})

test('deletes a block between stable markers', async () => {
  const facade = new FakeFacade()
  facade.existingText =
    '// lesson:start\nremove this line\n// lesson:end\nkeep this line\n'
  const deletion = deleteAction({
    target: textRange(
      '// lesson:start',
      '// lesson:end',
      '// lesson:start\nremove this line\n// lesson:end\n',
    ),
    speed_ms: 30,
  })

  await applyOpenedDelete(deletion, facade)

  assert.equal(facade.existingText, 'keep this line\n')
})

test('can delete an entire logical line including its break', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a\nb\nc\n'

  await applyOpenedDelete(
    deleteAction({
      target: {
        start: { line: 2 },
        end: { line: 2, placement: 'after' },
        expected_text: 'b\n',
      },
    }),
    facade,
  )

  assert.equal(facade.existingText, 'a\nc\n')
})

test('uses a literal index to delete one repeated match', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'same\nsame\n'

  await applyOpenedDelete(
    deleteAction({
      target: {
        start: { at: { text: 'same', index: 1 } },
        end: { at: { text: 'same', index: 1 }, placement: 'after' },
        expected_text: 'same\n',
      },
    }),
    facade,
  )

  assert.equal(facade.existingText, 'same\n')
})

test('rejects a cursor deletion that would replay on another occurrence', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'aaa'
  facade.target.savedText = 'aaa'

  await assert.rejects(
    () =>
      applyOpenedDelete(
        deleteAction({
          target: {
            cursor: { line: 1, column: 2, count: 1 },
            expected_text: 'a',
          },
          direction: 'backspace',
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'ambiguous_code_match',
  )
  assert.equal(facade.existingText, 'aaa')
  assert.equal(facade.replacements.length, 0)
})

test('rejects a range deletion that would replay on another occurrence', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'same\nsame\n'
  facade.target.savedText = 'same\nsame\n'

  await assert.rejects(
    () =>
      applyOpenedDelete(
        deleteAction({
          target: {
            start: { at: { text: 'same', index: 0 } },
            end: { at: { text: 'same', index: 0 }, placement: 'after' },
            expected_text: 'same\n',
          },
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'ambiguous_code_match',
  )
  assert.equal(facade.existingText, 'same\nsame\n')
  assert.equal(facade.replacements.length, 0)
})

test('deletes Unicode characters in the backspace direction', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'abc😀d'

  await applyOpenedDelete(
    deleteAction({
      target: {
        cursor: {
          line: 1,
          column: 4,
          count: 2,
        },
        expected_text: 'c😀',
      },
      direction: 'backspace',
    }),
    facade,
  )

  assert.equal(facade.existingText, 'abd')
})

test('deletes Unicode characters in the forward direction', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a😀bc'

  await applyOpenedDelete(
    deleteAction({
      target: {
        cursor: {
          line: 1,
          column: 1,
          count: 2,
        },
        expected_text: '😀b',
      },
      direction: 'delete',
    }),
    facade,
  )

  assert.equal(facade.existingText, 'ac')
})

test('rejects a deletion count beyond the document boundary', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'abc\n'

  await assert.rejects(
    () =>
      applyOpenedDelete(
        deleteAction({
          target: {
            cursor: {
              line: 1,
              column: 0,
              count: 1,
            },
            expected_text: 'a',
          },
          direction: 'backspace',
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'delete_out_of_range',
  )
})

test('restores the full document when deletion is cancelled', async () => {
  const original =
    'fn main() {\n    let temporary = 5;\n    println!("ok");\n}\n'
  const facade = new FakeFacade()
  facade.existingText = original
  const controller = new AbortController()
  const delay = facade.delay.bind(facade)
  facade.delay = async (ms) => {
    await delay(ms)
    controller.abort()
  }

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: textRange(
        '    let temporary = 5;\n    println!("ok");',
        undefined,
        '    let temporary = 5;\n    println!("ok");\n',
      ),
      speed_ms: 5,
    }),
    facade,
    controller.signal,
  )

  assert.equal(outcome, 'cancelled')
  assert.equal(facade.existingText, original)
  assert.equal(facade.saved, true)
})

test('rejects deletion and replacement in a dirty document', async () => {
  const deleteFacade = new FakeFacade()
  deleteFacade.existingText = 'temporary\n'
  await applyAction(
    { type: 'OpenFile', path: 'src/main.rs' },
    actionFacades(deleteFacade),
  )
  deleteFacade.target.dirty = true

  await assert.rejects(
    () => applyAction(deleteAction(), actionFacades(deleteFacade)),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'document_dirty',
  )

  const replaceFacade = new FakeFacade()
  replaceFacade.existingText = 'let x = 5;'
  await applyAction(
    { type: 'OpenFile', path: 'variables/main.rs' },
    actionFacades(replaceFacade),
  )
  replaceFacade.target.dirty = true

  await assert.rejects(
    () => applyAction(replaceAction(), actionFacades(replaceFacade)),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'document_dirty',
  )
})
