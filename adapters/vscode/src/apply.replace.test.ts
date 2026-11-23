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

test('applies a line-targeted correction to the intended repeated println', async () => {
  const source = [
    'fn main() {',
    '    let another_binding;',
    '',
    '    println!("another binding: {}", another_binding);',
    '    // 改正 ^ 注释掉此行',
    '',
    '    another_binding = 1;',
    '',
    '    println!("another binding: {}", another_binding);',
    '}',
    '',
  ].join('\n')
  const correction: ReplaceTextAction = {
    type: 'ReplaceText',
    path: 'examples/repeated_println.rs',
    target: {
      start: { line: 4 },
      end: { line: 4, placement: 'after' },
      expected_text: '    println!("another binding: {}", another_binding);\n',
    },
    text: '    // println!("another binding: {}", another_binding);\n',
    speed_ms: 1,
    edit_mode: 'delete_then_type',
  }
  const facade = new FakeFacade()
  facade.existingText = source

  await applyOpenedReplace(correction, facade)

  const println = '    println!("another binding: {}", another_binding);'
  assert.equal(facade.existingText.split(println).length - 1, 1)
  assert.equal(
    facade.existingText.split(`    // ${println.trimStart()}`).length - 1,
    1,
  )
  assert.equal(facade.saved, true)
})

test('replaces one local code fragment and saves the document', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\n    let x = 5;\n}\n'

  await applyOpenedReplace(replaceAction(), facade)

  assert.equal(facade.existingText, 'fn main() {\n    let mut x = 5;\n}\n')
  assert.equal(facade.saved, true)
})

test('treats a missing target as complete when the replacement is unique', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let mut x = 5;\n'

  const outcome = await applyOpenedReplace(replaceAction(), facade)

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'let mut x = 5;\n')
  assert.equal(facade.saved, true)
})

test('rejects a missing target when the expected text is still present', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let mut x = 5;'

  await assert.rejects(
    () =>
      applyOpenedReplace(
        replaceAction({
          target: textRange('let y = 5;', 'let y = 5;', 'x = 5;'),
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'code_match_missing',
  )
})

test('replays a completed replacement after its text anchors are gone', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'println!("{}", matrix);\n'

  const outcome = await applyOpenedReplace(
    replaceAction({
      target: textRange('println!("{:?}", matrix)'),
      text: 'println!("{}", matrix);',
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'println!("{}", matrix);\n')
  assert.equal(facade.saved, true)
})

test('replaces the first repeated text anchor when index is omitted', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let old = 1;\nlet old = 1;\n'

  await applyOpenedReplace(
    replaceAction({
      target: textRange('let old = 1;', undefined, 'let old = 1;\n'),
      text: 'let new = 2;\n',
    }),
    facade,
  )

  assert.equal(facade.existingText, 'let new = 2;\nlet old = 1;\n')
})

test('does not treat a unique replacement as complete when a coordinate anchor is out of range', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let mut x = 5;\n'

  await assert.rejects(
    () =>
      applyOpenedReplace(
        replaceAction({
          target: {
            start: { line: 9 },
            end: { line: 9, placement: 'after' },
            expected_text: 'let gone = 0;',
          },
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'line_out_of_range',
  )
})

test('treats a replacement whose selected target already equals the code as complete', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let mut x = 5;'

  const outcome = await applyOpenedReplace(
    replaceAction({
      target: textRange('let mut x = 5;'),
      text: 'let mut x = 5;',
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'let mut x = 5;')
  assert.equal(facade.saved, true)
})

test('rejects a local replacement when the source text is missing', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let y = 5;'

  await assert.rejects(
    () => applyOpenedReplace(replaceAction(), facade),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'code_match_missing',
  )
})

test('does not treat an ambiguous post-state as already applied', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let mut x = 5;\nlet mut x = 5;\n'

  await assert.rejects(
    () => applyOpenedReplace(replaceAction(), facade),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'code_match_missing',
  )
})

test('applies a local replacement to the first of several identical matches', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let x = 5;\nlet x = 5;\n'

  await applyOpenedReplace(replaceAction(), facade)

  assert.equal(facade.existingText, 'let mut x = 5;\nlet x = 5;\n')
})

test('matches local replacements across CRLF documents', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'fn main() {\r\n    let x = 5;\r\n}\r\n'

  await applyOpenedReplace(replaceAction(), facade)

  assert.equal(
    facade.existingText,
    'fn main() {\r\n    let mut x = 5;\r\n}\r\n',
  )
})
