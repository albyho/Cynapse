import assert from 'node:assert/strict'
import { test } from 'node:test'

import { parseCourseAction } from './actions'

function insertAction(target: unknown): Record<string, unknown> {
  return {
    type: 'InsertText',
    path: 'src/main.rs',
    target,
    text: '// ',
  }
}

function replaceAction(target: unknown): Record<string, unknown> {
  return {
    type: 'ReplaceText',
    path: 'src/main.rs',
    target,
    text: 'new',
  }
}

function range(
  start: Record<string, unknown>,
  end: Record<string, unknown>,
  expectedText = 'old',
): Record<string, unknown> {
  return { start, end, expected_text: expectedText }
}

test('accepts every valid anchor shape and rejects malformed ones', () => {
  const valid = [
    { line: 1 },
    { line: 1, placement: 'after' },
    { line: 1, at: 0 },
    { line: 1, at: 3, placement: 'after' },
    { at: { text: 'fn main' } },
    { line: 2, at: { text: 'x', index: 1 } },
    { line: 2, at: { text: 'x', index: 1 }, placement: 'before' },
    { eof: true },
    { eof: false, line: 1 },
  ]
  for (const target of valid) {
    assert.notEqual(
      parseCourseAction(insertAction(target)),
      undefined,
      JSON.stringify(target),
    )
  }

  const invalid = [
    {},
    { line: 0 },
    { line: 1.5 },
    { line: '1' },
    { at: 2 },
    { line: 1, at: -1 },
    { line: 1, at: 1.5 },
    { at: { text: '' } },
    { at: { text: 'x', index: null } },
    { at: { text: 'x', index: -1 } },
    { at: { text: 'x', extra: 1 } },
    { at: { text: 'x', index: 0, placement: 'after' } },
    { at: 'fn main' },
    { line: 1, at: { text: 'x' }, extra: 1 },
    { line: 1, placement: 'middle' },
    { eof: true, line: 1 },
    { eof: true, at: 0 },
    { eof: true, placement: 'before' },
    { line: null },
    { at: null },
    { placement: null },
    { eof: null },
    // Old anchor shapes.
    { kind: 'line', line: 1 },
    { kind: 'position', line: 1, column: 2 },
    { kind: 'literal', text: 'fn main', placement: 'before' },
    { kind: 'end_of_file' },
    { line: 1, column: 2 },
    { text: 'fn main', placement: 'before' },
    { line: 1, text: 'fn main' },
  ]
  for (const target of invalid) {
    assert.equal(
      parseCourseAction(insertAction(target)),
      undefined,
      JSON.stringify(target),
    )
  }
})

test('validates every text range placement pair and point range', () => {
  const valid = [
    // A whole-line range keeps the default start-before/end-after pair.
    [range({ line: 1 }, { line: 1 }, 'line\n')],
    [range({ line: 1 }, { line: 2 }, 'block\n')],
    [range({ line: 1 }, { line: 1, at: 0 })],
    [range({ line: 1, at: 0 }, { line: 1, at: 3 })],
    [range({ line: 1, at: 3 }, { line: 1 })],
    [range({ line: 1 }, { eof: true })],
    [range({ at: { text: 'old' } }, { line: 2 })],
    [range({ line: 1 }, { at: { text: 'new' } })],
    [range({ at: { text: 'old' } }, { at: { text: 'old' } })],
    // All four text placement combinations are legal for distinct anchors.
    [
      range(
        { at: { text: 'old' }, placement: 'before' },
        { at: { text: 'new' }, placement: 'before' },
      ),
    ],
    [
      range(
        { at: { text: 'old' }, placement: 'before' },
        { at: { text: 'new' }, placement: 'after' },
      ),
    ],
    [
      range(
        { at: { text: 'old' }, placement: 'after' },
        { at: { text: 'new' }, placement: 'before' },
      ),
    ],
    [
      range(
        { at: { text: 'old' }, placement: 'after' },
        { at: { text: 'new' }, placement: 'after' },
      ),
    ],
    [
      range(
        { line: 1, at: { text: 'old', index: 0 } },
        { line: 2, at: { text: 'new', index: 0 } },
      ),
    ],
  ]
  for (const [target] of valid) {
    assert.notEqual(
      parseCourseAction(replaceAction(target)),
      undefined,
      JSON.stringify(target),
    )
  }

  const invalid = [
    // Same line + text + index with start after or end before is empty.
    [
      range(
        { at: { text: 'old' }, placement: 'after' },
        { at: { text: 'old' }, placement: 'after' },
      ),
    ],
    [
      range(
        { at: { text: 'old' }, placement: 'before' },
        { at: { text: 'old' }, placement: 'before' },
      ),
    ],
    [
      range(
        { at: { text: 'old' }, placement: 'after' },
        { at: { text: 'old' }, placement: 'before' },
      ),
    ],
    // Point ranges must move forward.
    [range({ line: 2 }, { line: 1 })],
    [range({ line: 1, at: 3 }, { line: 1, at: 2 })],
    // Start eof is invalid.
    [range({ eof: true }, { line: 1 })],
    // Malformed ranges.
    [{ start: { line: 1 }, end: { line: 2 } }],
    [
      {
        start: { line: 1 },
        end: { line: 2 },
        expected_text: '',
      },
    ],
    [
      {
        start: { line: 1 },
        end: { line: 2 },
        expected_text: 'old',
        include_end_line_break: true,
      },
    ],
    [
      {
        start: { line: 1 },
        end: { line: 2 },
        expected_text: 'old',
        kind: 'range',
      },
    ],
    [
      {
        start: { text: 'old' },
        end: { text: 'old' },
        expected_text: 'old',
      },
    ],
    [
      {
        start: { line: 1 },
        end: { text: 'old' },
        expected_text: 'old',
      },
    ],
    [
      {
        start: { line: 1, column: 0 },
        end: { line: 1, column: 2 },
        expected_text: 'old',
      },
    ],
  ]
  for (const [target] of invalid) {
    assert.equal(
      parseCourseAction(replaceAction(target)),
      undefined,
      JSON.stringify(target),
    )
  }
})

test('rejects old InsertText, DeleteText and ReplaceText shapes', () => {
  const oldInsertTargets = [
    { kind: 'line', line: 1 },
    { kind: 'position', line: 1, column: 2 },
    { kind: 'literal', text: 'old', placement: 'before' },
    { kind: 'end_of_file' },
  ]
  for (const target of oldInsertTargets) {
    assert.equal(parseCourseAction(insertAction(target)), undefined)
  }

  for (const target of [
    {
      start: { text: 'old' },
      end: { text: 'old' },
      expected_text: 'old',
    },
    {
      start: { line: 1, column: 0 },
      end: { line: 1, column: 2 },
      expected_text: 'old',
    },
    {
      start: { line: 1 },
      end: { line: 1 },
      expected_text: 'old',
      include_end_line_break: true,
    },
  ]) {
    assert.equal(
      parseCourseAction({
        type: 'DeleteText',
        path: 'src/main.rs',
        target,
      }),
      undefined,
      JSON.stringify(target),
    )
    assert.equal(parseCourseAction(replaceAction(target)), undefined)
  }
})
