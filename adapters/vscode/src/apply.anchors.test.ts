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

test('replaces an expected line range', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\n    let x = 5;\n}\n'

  await applyOpenedReplace(
    replaceAction({
      target: {
        start: { line: 2 },
        end: { line: 2, placement: 'after' },
        expected_text: '    let x = 5;\n',
      },
      text: '    let mut x = 5;\n',
    }),
    facade,
  )

  assert.equal(facade.existingText, 'fn main() {\n    let mut x = 5;\n}\n')
})

test('stops when an expected line changed before replacement', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\n    let x = 6;\n}\n'

  await assert.rejects(
    () =>
      applyOpenedReplace(
        replaceAction({
          target: {
            start: { line: 2 },
            end: { line: 2, placement: 'after' },
            expected_text: '    let x = 5;',
          },
          text: '    let mut x = 5;',
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'stale_edit_range',
  )
})

test('replaces a block between unique markers', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'before\nfn main() {\n    let x = 5;\n}\nafter\n'

  await applyOpenedReplace(
    replaceAction({
      target: textRange('fn main() {', '}', 'fn main() {\n    let x = 5;\n}\n'),
      text: 'fn main() {\n    let mut x = 5;\n}\n',
    }),
    facade,
  )

  assert.equal(
    facade.existingText,
    'before\nfn main() {\n    let mut x = 5;\n}\nafter\n',
  )
})

test('replaces across mixed coordinate and text anchors', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\n    let x = 5;\n    return;\n}\n'

  await applyOpenedReplace(
    replaceAction({
      target: {
        start: { line: 2, at: 4 },
        end: { at: { text: 'return' }, placement: 'before' },
        expected_text: 'let x = 5;\n    ',
      },
      text: 'let mut x = 6;\n    ',
    }),
    facade,
  )

  assert.equal(
    facade.existingText,
    'fn main() {\n    let mut x = 6;\n    return;\n}\n',
  )
})

test('deletes from a text anchor through a coordinate line end', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'before\nkeep\nremove one\nremove two\nafter\n'

  await applyOpenedDelete(
    deleteAction({
      target: {
        start: { at: { text: 'remove one' } },
        end: { line: 4, placement: 'after' },
        expected_text: 'remove one\nremove two\n',
      },
    }),
    facade,
  )

  assert.equal(facade.existingText, 'before\nkeep\nafter\n')
})

test('deletes between repeated text using explicit anchor indices', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'foo\nmiddle\nfoo'

  await applyOpenedDelete(
    deleteAction({
      target: {
        start: { at: { text: 'foo', index: 0 }, placement: 'after' },
        end: { at: { text: 'foo', index: 1 }, placement: 'before' },
        expected_text: '\nmiddle\n',
      },
    }),
    facade,
  )

  assert.equal(facade.existingText, 'foofoo')
})

test('rejects an expected_text that omits the line-end break', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a\nb\n'

  await assert.rejects(
    () =>
      applyOpenedReplace(
        replaceAction({
          target: {
            start: { line: 1 },
            end: { line: 1, placement: 'after' },
            expected_text: 'a',
          },
          text: 'A',
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'stale_edit_range',
  )
})

test('scopes a text anchor to one line and indexes its occurrences', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'x = 1;\nx = x;\n'

  await applyOpenedInsert(
    insertAction({
      target: { line: 2, at: { text: 'x', index: 1 }, placement: 'after' },
      text: '!',
    }),
    facade,
  )

  assert.equal(facade.existingText, 'x = 1;\nx = x!;\n')
})

test('uses the first text match when index is omitted', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'x = x;\n'

  await applyOpenedInsert(
    insertAction({
      target: { at: { text: 'x' }, placement: 'after' },
      text: '!',
    }),
    facade,
  )

  assert.equal(facade.existingText, 'x! = x;\n')
})

test('accepts eof as a range end but not as a range start', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a\nb\n'

  await applyOpenedDelete(
    deleteAction({
      target: {
        start: { line: 2, at: 0 },
        end: { eof: true },
        expected_text: 'b\n',
      },
    }),
    facade,
  )
  assert.equal(facade.existingText, 'a\n')

  await assert.rejects(
    () =>
      applyOpenedDelete(
        deleteAction({
          target: {
            start: { eof: true },
            end: { line: 1 },
            expected_text: 'a',
          },
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'invalid_edit_range',
  )
})

test('a line-end range end includes the following break by default', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a\nb\n'

  await applyOpenedReplace(
    replaceAction({
      target: {
        start: { line: 1 },
        end: { line: 1 },
        expected_text: 'a\n',
      },
      text: 'A\n',
    }),
    facade,
  )

  assert.equal(facade.existingText, 'A\nb\n')
})

test('an empty-line column end includes the following break', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a\n\nb'

  await applyOpenedDelete(
    deleteAction({
      target: {
        start: { line: 2, at: 0, placement: 'after' },
        end: { line: 2, at: 0, placement: 'after' },
        expected_text: '\n',
      },
    }),
    facade,
  )

  assert.equal(facade.existingText, 'a\nb')
})

test('insert after a line break uses the next line before anchor', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a\nb'

  await applyOpenedInsert(
    insertAction({ target: { line: 2 }, text: '!' }),
    facade,
  )

  assert.equal(facade.existingText, 'a\n!b')
})

test('insert after a line end stays before that line break', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'a\nb'

  await applyOpenedInsert(
    insertAction({
      target: { line: 1, placement: 'after' },
      text: '!',
    }),
    facade,
  )
  assert.equal(facade.existingText, 'a!\nb')

  await applyOpenedInsert(
    insertAction({ target: { line: 1, at: 2 }, text: '?' }),
    facade,
  )
  assert.equal(facade.existingText, 'a!?\nb')
})

test('selects all four text placement pairs for distinct anchors', async () => {
  const cases = [
    ['before', 'before', 'abc ', 'X', 'Xabc\n'],
    ['before', 'after', 'abc abc\n', 'X\n', 'X\n'],
    ['after', 'before', ' ', 'X', 'abcXabc\n'],
    ['after', 'after', ' abc\n', 'X\n', 'abcX\n'],
  ] as const

  for (const [start, end, expected, text, terminal] of cases) {
    const facade = new FakeFacade()
    facade.existingText = 'abc abc\n'

    await applyOpenedReplace(
      replaceAction({
        target: {
          start: { at: { text: 'abc', index: 0 }, placement: start },
          end: { at: { text: 'abc', index: 1 }, placement: end },
          expected_text: expected,
        },
        text,
      }),
      facade,
    )

    assert.equal(facade.existingText, terminal, `${start}/${end}`)
  }
})
