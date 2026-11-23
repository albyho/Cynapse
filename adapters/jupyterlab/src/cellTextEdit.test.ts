import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  cellLineEnding,
  planCellDeleteText,
  planCellInsertText,
  planCellReplaceText,
  withLineEnding,
} from './cellTextEdit'

test('inserts with a line-scoped, indexed text anchor', () => {
  const plan = planCellInsertText(
    'x = 1;\nx = x;\n',
    { line: 2, at: { text: 'x', index: 1 }, placement: 'after' },
    '!',
  )

  assert.equal(plan.next, 'x = 1;\nx = x!;\n')
  assert.equal(plan.start, 12)
})

test('an omitted text index selects the first occurrence', () => {
  const plan = planCellInsertText(
    'x = x;\n',
    { at: { text: 'x' }, placement: 'after' },
    '!',
  )

  assert.equal(plan.next, 'x! = x;\n')
})

test('an explicit text index selects a later occurrence', () => {
  assert.equal(
    planCellInsertText('x = x;\n', { at: { text: 'x', index: 1 } }, '!').next,
    'x = !x;\n',
  )
  assert.throws(
    () => planCellInsertText('x = x;\n', { at: { text: 'x', index: 2 } }, '!'),
    /not found/,
  )
  assert.throws(
    () => planCellInsertText('x = x;\n', { at: { text: 'missing' } }, '!'),
    /not found/,
  )
})

test('inserts before and after a line break without crossing it', () => {
  assert.equal(
    planCellInsertText('a\nb', { line: 1, placement: 'after' }, '!').next,
    'a!\nb',
  )
  assert.equal(planCellInsertText('a\nb', { line: 2 }, '!').next, 'a\n!b')
  assert.equal(
    planCellInsertText('a\nb', { at: { text: 'a' }, placement: 'after' }, '!')
      .next,
    'a!\nb',
  )
  assert.equal(planCellInsertText('a\nb', { eof: true }, '!').next, 'a\nb!')
})

test('keeps column inserts at a line end before the line break', () => {
  assert.equal(
    planCellInsertText('abc\nd', { line: 1, at: 1 }, '!').next,
    'a!bc\nd',
  )
  assert.equal(
    planCellInsertText('abc\nd', { line: 1, at: 2, placement: 'after' }, '!')
      .next,
    'abc!\nd',
  )
  assert.equal(
    planCellInsertText('abc\nd', { line: 1, at: 3, placement: 'after' }, '!')
      .next,
    'abc!\nd',
  )
  assert.throws(
    () => planCellInsertText('abc\nd', { line: 1, at: 4 }, '!'),
    /outside line/,
  )
})

test('a range end at a line end includes the following line break', () => {
  assert.equal(
    planCellReplaceText(
      'a\nb\n',
      {
        start: { line: 1 },
        end: { line: 1 },
        expected_text: 'a\n',
      },
      'A\n',
    ).next,
    'A\nb\n',
  )
  assert.throws(
    () =>
      planCellReplaceText(
        'a\nb\n',
        {
          start: { line: 1 },
          end: { line: 1 },
          expected_text: 'a',
        },
        'A',
      ),
    /expected_text/,
  )
  assert.equal(
    planCellDeleteText('abc\ndef\n', {
      start: { line: 1, at: 0 },
      end: { line: 1, at: 3 },
      expected_text: 'abc\n',
    }).next,
    'def\n',
  )
  assert.equal(
    planCellDeleteText('abc\ndef\n', {
      start: { at: { text: 'abc' } },
      end: { at: { text: 'abc' }, placement: 'after' },
      expected_text: 'abc\n',
    }).next,
    'def\n',
  )
  assert.throws(
    () =>
      planCellDeleteText('abc\ndef\n', {
        start: { at: { text: 'abc' } },
        end: { at: { text: 'abc' }, placement: 'before' },
        expected_text: 'abc\n',
      }),
    /before its end/,
  )
})

test('an omitted placement is role-aware', () => {
  assert.equal(planCellInsertText('a\nb', { line: 2 }, '!').next, 'a\n!b')
  assert.equal(
    planCellDeleteText('a\nb\n', {
      start: { line: 2 },
      end: { line: 2 },
      expected_text: 'b\n',
    }).next,
    'a\n',
  )
  assert.equal(
    planCellDeleteText('abc\n', {
      start: { line: 1, at: 0 },
      end: { line: 1, at: 1 },
      expected_text: 'a',
    }).next,
    'bc\n',
  )
  assert.equal(
    planCellDeleteText('abc\ndef\n', {
      start: { at: { text: 'abc' } },
      end: { at: { text: 'abc' } },
      expected_text: 'abc\n',
    }).next,
    'def\n',
  )
})

test('an after column end on an empty line includes the line break', () => {
  const plan = planCellDeleteText('a\n\nb', {
    start: { line: 2, at: 0, placement: 'after' },
    end: { line: 2, at: 0, placement: 'after' },
    expected_text: '\n',
  })

  assert.equal(plan.next, 'a\nb')
})

test('a range end at a line start does not add a line break', () => {
  const plan = planCellDeleteText('a\nb\n', {
    start: { line: 1, at: 1 },
    end: { line: 2, placement: 'before' },
    expected_text: '\n',
  })

  assert.equal(plan.next, 'ab\n')
})

test('applies every text start and end placement combination', () => {
  const cases = [
    ['before', 'after', 'old new', '  X  \n'],
    ['after', 'before', ' ', '  oldXnew  \n'],
    ['before', 'before', 'old ', '  Xnew  \n'],
    ['after', 'after', ' new', '  oldX  \n'],
  ] as const
  for (const [startPlacement, endPlacement, expected, next] of cases) {
    const plan = planCellReplaceText(
      '  old new  \n',
      {
        start: { at: { text: 'old' }, placement: startPlacement },
        end: { at: { text: 'new' }, placement: endPlacement },
        expected_text: expected,
      },
      'X',
    )
    assert.equal(plan.next, next, `${startPlacement}/${endPlacement}`)
  }
})

test('accepts an eof range end but rejects an eof range start', () => {
  assert.equal(
    planCellDeleteText('a\nb\n', {
      start: { line: 2 },
      end: { eof: true },
      expected_text: 'b\n',
    }).next,
    'a\n',
  )
  assert.throws(
    () =>
      planCellDeleteText('a\nb\n', {
        start: { eof: true },
        end: { eof: true },
        expected_text: 'b\n',
      }),
    /eof/,
  )
})

test('normalizes CRLF documents and expected text', () => {
  assert.equal(
    planCellInsertText('a\r\nb\r\n', { line: 2, at: 0 }, 'c\r\n').next,
    'a\nc\nb\n',
  )
  assert.equal(
    planCellReplaceText(
      'a\r\nb\r\n',
      {
        start: { line: 1 },
        end: { line: 1, placement: 'after' },
        expected_text: 'a\r\n',
      },
      'A\r\n',
    ).next,
    'A\nb\n',
  )
})

test('preserves the cell line endings when one range is edited', () => {
  const source = 'one\r\ntwo\r\nthree\r\n'
  const plan = planCellReplaceText(
    source,
    { start: { line: 2 }, end: { line: 2 }, expected_text: 'two\r\n' },
    'TWO\r\n',
  )

  assert.equal(plan.next, 'one\nTWO\nthree\n')
  assert.equal(
    withLineEnding(plan.next, cellLineEnding(source)),
    'one\r\nTWO\r\nthree\r\n',
  )
  assert.equal(cellLineEnding('a\nb\nc\r\n'), '\n')
  assert.equal(cellLineEnding('a\r\nb\r\nc\n'), '\r\n')
})
