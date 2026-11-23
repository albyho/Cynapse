import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  ApplyError,
  resolveInsertPosition,
  type InsertTextAction,
} from './apply'
import {
  FakeFacade,
  replaceAction,
  deleteAction,
  textRange,
  applyOpenedReplace,
  applyOpenedDelete,
  applyOpenedInsert,
  insertAction,
  crashedFacade,
  assertSelfHealed,
  assertDirtyRejected,
} from './testSupport'

// ---------------------------------------------------------------------------
// Recovery self-heal: a crashed run leaves its own partial product in a dirty
// buffer while the disk file keeps the pre-Action baseline. On retry the edit
// Action must converge the document to its terminal state in one instant edit
// with no typewriter animation, and reject any buffer whose diff is not its
// own product. See ADR-0039 and issue #74.
// ---------------------------------------------------------------------------

test('applies the full insertion instantly when the crash left nothing typed', async () => {
  const baseline = 'fn main() {\n}\n'
  const facade = crashedFacade(baseline, baseline)

  const outcome = await applyOpenedInsert(
    insertAction({
      target: { line: 2, at: 0 },
      text: '    println!("ok");\n',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'fn main() {\n    println!("ok");\n}\n', 1)
  assert.equal(
    facade.inserts.map((insert) => insert.text).join(''),
    '    println!("ok");\n',
  )
})

test('saves without editing when the crashed buffer already holds the complete insertion', async () => {
  const terminal = 'fn main() {\n    println!("ok");\n}\n'
  const facade = crashedFacade('fn main() {\n}\n', terminal)

  const outcome = await applyOpenedInsert(
    insertAction({
      target: { line: 2, at: 0 },
      text: '    println!("ok");\n',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, terminal, 0)
})

test('rejects a crashed buffer whose diff is not the insertion product', async () => {
  const baseline = 'fn main() {\n}\n'
  const buffer = 'fn main() {\nUSER}\n'
  const facade = crashedFacade(baseline, buffer)

  await assert.rejects(
    () =>
      applyOpenedInsert(
        insertAction({
          target: { line: 2, at: 0 },
          text: '    println!("ok");\n',
          speed_ms: 30,
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'document_dirty',
  )
  assertDirtyRejected(facade, buffer)
})

test('self-heals a half-consumed backspace range deletion from a crashed dirty buffer', async () => {
  const baseline = 'keep temporary end\n'
  const facade = crashedFacade(baseline, 'keep temp end\n')

  const outcome = await applyOpenedDelete(
    deleteAction({
      direction: 'backspace',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'keep  end\n', 1)
  assert.deepEqual(facade.replacements, [
    {
      start: { line: 0, character: 5 },
      end: { line: 0, character: 9 },
      text: '',
    },
  ])
})

test('self-heals a half-consumed forward range deletion from a crashed dirty buffer', async () => {
  const baseline = 'keep temporary end\n'
  const facade = crashedFacade(baseline, 'keep orary end\n')

  const outcome = await applyOpenedDelete(
    deleteAction({
      direction: 'delete',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'keep  end\n', 1)
})

test('deletes the full range instantly when the crash left nothing consumed', async () => {
  const baseline = 'keep temporary end\n'
  const facade = crashedFacade(baseline, baseline)

  const outcome = await applyOpenedDelete(
    deleteAction({
      direction: 'backspace',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'keep  end\n', 1)
})

test('saves without editing when the crashed buffer already holds the completed deletion', async () => {
  const baseline = 'keep temporary end\n'
  const facade = crashedFacade(baseline, 'keep  end\n')

  const outcome = await applyOpenedDelete(
    deleteAction({
      direction: 'backspace',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'keep  end\n', 0)
})

test('self-heals a half-consumed cursor backspace deletion from a crashed dirty buffer', async () => {
  const baseline = 'let abc = 1;\n'
  const facade = crashedFacade(baseline, 'let a = 1;\n')

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: {
        cursor: { line: 1, column: 7, count: 4 },
        expected_text: ' abc',
      },
      direction: 'backspace',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'let = 1;\n', 1)
})

test('self-heals a half-consumed cursor forward deletion from a crashed dirty buffer', async () => {
  const baseline = 'let abc = 1;\n'
  const facade = crashedFacade(baseline, 'let abc1;\n')

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: {
        cursor: { line: 1, column: 7, count: 4 },
        expected_text: ' = 1',
      },
      direction: 'delete',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'let abc;\n', 1)
})

test('skips a completed cursor deletion on a clean disk', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'let = 1;\n'
  facade.target.savedText = 'let = 1;\n'

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: {
        cursor: { line: 1, column: 7, count: 4 },
        expected_text: ' abc',
      },
      direction: 'backspace',
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'let = 1;\n')
  assert.equal(facade.saves, 1)
  assert.equal(facade.replacements.length, 0)
})

test('skips a completed cursor deletion when the expected text still appears elsewhere', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'xy : z\n'
  facade.target.savedText = 'xy : z\n'

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: {
        cursor: { line: 1, column: 1, count: 1 },
        expected_text: ':',
      },
      direction: 'delete',
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'xy : z\n')
  assert.equal(facade.saves, 1)
  assert.equal(facade.replacements.length, 0)
})

test('treats a range whose anchors moved onto other text as completed', async () => {
  const facade = new FakeFacade()
  facade.existingText = ':xy'
  facade.target.savedText = ':xy'

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: {
        start: { line: 1, at: 0 },
        end: { line: 1, at: 1 },
        expected_text: 'x',
      },
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, ':xy')
  assert.equal(facade.saves, 1)
  assert.equal(facade.replacements.length, 0)
})

test('treats adjacent delete anchors as an already deleted range', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'keepend\n'
  facade.target.savedText = 'keepend\n'

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: {
        start: { at: { text: 'keep' }, placement: 'after' },
        end: { at: { text: 'end' }, placement: 'before' },
        expected_text: ' temporary ',
      },
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'keepend\n')
  assert.equal(facade.saves, 1)
  assert.equal(facade.replacements.length, 0)
})

test('treats a completed coordinate range deletion as already done', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'keep  end\n'
  facade.target.savedText = 'keep  end\n'

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: {
        start: { line: 1, at: 5 },
        end: { line: 1, at: 14 },
        expected_text: ' temporary ',
      },
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'keep  end\n')
  assert.equal(facade.saves, 1)
  assert.equal(facade.replacements.length, 0)
})

test('treats a completed text-anchor deletion with missing anchors as already done', async () => {
  const facade = new FakeFacade()
  facade.existingText = 'keep  end\n'
  facade.target.savedText = 'keep  end\n'

  const outcome = await applyOpenedDelete(
    deleteAction({
      target: textRange('temporary'),
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'keep  end\n')
  assert.equal(facade.saves, 1)
  assert.equal(facade.replacements.length, 0)
})

test('does not save a later action partial after a completed range deletion', async () => {
  const facade = crashedFacade('keep  end\n', 'keep  end\nUSER')
  const outcome = await applyOpenedDelete(
    deleteAction({
      target: textRange('temporary'),
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.equal(facade.existingText, 'keep  end\nUSER')
  assert.equal(facade.saves, 0)
  assert.equal(facade.replacements.length, 0)
})

test('rejects a crashed buffer whose diff is not the deletion product', async () => {
  const baseline = 'keep temporary end\n'
  const buffer = 'keep temporaryX end\n'
  const facade = crashedFacade(baseline, buffer)

  await assert.rejects(
    () =>
      applyOpenedDelete(
        deleteAction({
          direction: 'backspace',
          speed_ms: 30,
        }),
        facade,
      ),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'document_dirty',
  )
  assertDirtyRejected(facade, buffer)
})

test('self-heals a half-typed overwrite replacement from a crashed dirty buffer', async () => {
  const baseline = 'let x = 5;\n'
  const facade = crashedFacade(baseline, 'let mut ')

  const outcome = await applyOpenedReplace(
    replaceAction({ speed_ms: 30 }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'let mut x = 5;\n', 1)
  assert.deepEqual(facade.replacements, [
    {
      start: { line: 0, character: 0 },
      end: { line: 0, character: 8 },
      text: 'let mut x = 5;\n',
    },
  ])
})

test('self-heals a delete-then-type replacement interrupted while deleting', async () => {
  const baseline = 'let x = 5;\n'
  const facade = crashedFacade(baseline, 'let x = 5')

  const outcome = await applyOpenedReplace(
    replaceAction({
      edit_mode: 'delete_then_type',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'let mut x = 5;\n', 1)
})

test('self-heals a delete-then-type replacement interrupted while typing', async () => {
  const baseline = 'let x = 5;\n'
  const facade = crashedFacade(baseline, 'let m')

  const outcome = await applyOpenedReplace(
    replaceAction({
      edit_mode: 'delete_then_type',
      speed_ms: 30,
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'let mut x = 5;\n', 1)
})

test('replaces the full range instantly when the crash left nothing applied', async () => {
  const baseline = 'let x = 5;\n'
  const facade = crashedFacade(baseline, baseline)

  const outcome = await applyOpenedReplace(
    replaceAction({ speed_ms: 30 }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'let mut x = 5;\n', 1)
})

test('saves without editing when the crashed buffer already holds the completed replacement', async () => {
  const baseline = 'let x = 5;\n'
  const facade = crashedFacade(baseline, 'let mut x = 5;\n')

  const outcome = await applyOpenedReplace(
    replaceAction({ speed_ms: 30 }),
    facade,
  )

  assert.equal(outcome, 'done')
  assertSelfHealed(facade, 'let mut x = 5;\n', 0)
})

test('rejects a crashed buffer whose diff is not the replacement product', async () => {
  const baseline = 'let x = 5;\n'
  const buffer = 'let USER = 5;\n'
  const facade = crashedFacade(baseline, buffer)

  await assert.rejects(
    () => applyOpenedReplace(replaceAction({ speed_ms: 30 }), facade),
    (error: unknown) =>
      error instanceof ApplyError && error.code === 'document_dirty',
  )
  assertDirtyRejected(facade, buffer)
})

test('skips a completed insertion without saving when a later Action crashed mid-edit', async () => {
  const disk = 'fn main() {\n    println!("old");\n}\n'
  const buffer = 'fn main() {\n    println!("ne;\n}\n'
  const facade = crashedFacade(disk, buffer)

  const insertOutcome = await applyOpenedInsert(
    insertAction({
      target: { line: 2, at: 0 },
      text: '    println!("old");\n',
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(insertOutcome, 'done')
  assert.equal(facade.inserts.length, 0)
  assert.equal(
    facade.saves,
    0,
    'a skipped Action must not save the dirty buffer',
  )

  const replaceOutcome = await applyOpenedReplace(
    replaceAction({
      target: textRange('println!("old")'),
      text: 'println!("new")',
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(replaceOutcome, 'done')
  assertSelfHealed(facade, 'fn main() {\n    println!("new");\n}\n', 1)
})

test('skips a completed deletion without saving when a later Action crashed mid-edit', async () => {
  const disk = 'fn main() {\n    println!("old");\n}\n'
  const buffer = 'fn main() {\n    println!("ne;\n}\n'
  const facade = crashedFacade(disk, buffer)

  const deleteOutcome = await applyOpenedDelete(
    deleteAction({
      target: {
        start: { at: { text: 'fn main() {\n' }, placement: 'after' },
        end: { at: { text: '    println!("old");' }, placement: 'before' },
        expected_text: '    // temporary\n',
      },
    }),
    facade,
  )
  assert.equal(deleteOutcome, 'done')
  assert.equal(facade.replacements.length, 0)
  assert.equal(
    facade.saves,
    0,
    'a skipped Action must not save the dirty buffer',
  )

  const replaceOutcome = await applyOpenedReplace(
    replaceAction({
      target: textRange('println!("old")'),
      text: 'println!("new")',
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(replaceOutcome, 'done')
  assertSelfHealed(facade, 'fn main() {\n    println!("new");\n}\n', 1)
})

test('skips a completed replacement without saving when a later Action crashed mid-edit', async () => {
  const disk = 'let mut x = 5;\n'
  // The first run consumes the newline overlap and inserts `// tail\n`.
  const buffer = 'let mut x = 5;\n// t'
  const facade = crashedFacade(disk, buffer)

  const replaceOutcome = await applyOpenedReplace(
    replaceAction({
      target: {
        start: { line: 1, at: 0 },
        end: { line: 1, placement: 'after' },
        expected_text: 'let x = 5;\n',
      },
      text: 'let mut x = 5;\n',
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(replaceOutcome, 'done')
  assert.equal(facade.replacements.length, 0)
  assert.equal(
    facade.saves,
    0,
    'a skipped Action must not save the dirty buffer',
  )

  const insertOutcome = await applyOpenedInsert(
    insertAction({
      target: { eof: true },
      text: '\n// tail\n',
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(insertOutcome, 'done')
  assertSelfHealed(facade, 'let mut x = 5;\n// tail\n', 1)
})

test('keeps animating and saving normally on a clean disk (no recovery hijack)', async () => {
  const disk = 'fn main() {\n    println!("old");\n}\n'
  const facade = new FakeFacade()
  facade.existingText = disk
  facade.target.savedText = disk
  facade.target.dirty = false

  const insertOutcome = await applyOpenedInsert(
    insertAction({
      target: { eof: true },
      text: '\n// tail\n',
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(insertOutcome, 'done')
  assert.ok(facade.delays.length > 0, 'a clean first play must animate')
  // end_of_file continuation: the disk's trailing newline overlaps the code
  // prefix and is not re-typed (existing suffix-prefix semantics).
  assert.equal(facade.existingText, disk + '// tail\n')

  const deleteOutcome = await applyOpenedDelete(
    deleteAction({
      target: textRange(
        '    println!("old");\n',
        undefined,
        '    println!("old");\n',
      ),
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(deleteOutcome, 'done')
  assert.ok(facade.delays.length > 0, 'a clean first play must animate')
  assert.equal(facade.existingText, 'fn main() {\n}\n// tail\n')

  const replaceOutcome = await applyOpenedReplace(
    replaceAction({
      target: textRange('fn main() {', undefined, 'fn main() {\n'),
      text: 'fn main() {}\n',
      speed_ms: 30,
    }),
    facade,
  )
  assert.equal(replaceOutcome, 'done')
  assert.ok(facade.delays.length > 0, 'a clean first play must animate')
  assert.equal(facade.existingText, 'fn main() {}\n}\n// tail\n')
})

const insertCases: Array<{
  name: string
  target: InsertTextAction['target']
  baseline: string
  partial: string
  terminal: string
  text: string
  recoveredInsert?: string
}> = [
  {
    name: 'line',
    target: { line: 1 },
    baseline: 'let x = 5;\n',
    text: '// header\n',
    partial: '// hea',
    terminal: '// header\nlet x = 5;\n',
  },
  {
    name: 'position',
    target: { line: 2, at: 0 },
    baseline: 'fn main() {\n}\n',
    text: '    println!("ok");\n',
    partial: '    print',
    terminal: 'fn main() {\n    println!("ok");\n}\n',
  },
  {
    name: 'literal after',
    target: { at: { text: 'fn main' }, placement: 'after' },
    baseline: 'fn main\n',
    text: '() {}\n',
    partial: '() {',
    terminal: 'fn main() {}\n\n',
  },
  {
    name: 'literal before',
    target: { at: { text: 'let y' } },
    baseline: 'let y = 2;\n',
    text: '// note\n',
    partial: '// no',
    terminal: '// note\nlet y = 2;\n',
  },
  {
    name: 'end of file',
    target: { eof: true },
    baseline: 'fn main() {}\n',
    text: '\n// tail\n',
    // The first run consumes the leading newline overlap and inserts only
    // `// tail\n`; this is the real dirty-buffer shape.
    partial: '// t',
    terminal: 'fn main() {}\n// tail\n',
    recoveredInsert: 'ail\n',
  },
]

for (const insertCase of insertCases) {
  test(`self-heals a half-typed ${insertCase.name} insertion from a crashed dirty buffer`, async () => {
    const offset = resolveInsertOffsetForTest(
      insertCase.baseline,
      insertCase.target,
    )
    const facade = crashedFacade(
      insertCase.baseline,
      insertCase.baseline.slice(0, offset) +
        insertCase.partial +
        insertCase.baseline.slice(offset),
    )

    const outcome = await applyOpenedInsert(
      insertAction({
        target: insertCase.target,
        text: insertCase.text,
        speed_ms: 30,
      }),
      facade,
    )

    assert.equal(outcome, 'done')
    assertSelfHealed(facade, insertCase.terminal, 1)
    assert.equal(
      facade.inserts.map((insert) => insert.text).join(''),
      insertCase.recoveredInsert ??
        insertCase.text.slice(insertCase.partial.length),
    )
  })
}

// Delegates to the production resolution so the crash-buffer fixtures can
// never drift from the real insert position semantics.
function resolveInsertOffsetForTest(
  text: string,
  target: InsertTextAction['target'],
): number {
  return resolveInsertPosition(text, target).offset
}
