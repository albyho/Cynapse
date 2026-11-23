import assert from 'node:assert/strict'
import { test } from 'node:test'

import { parseCheckPayload } from './check'

test('applies shared check defaults', () => {
  assert.deepEqual(
    parseCheckPayload({
      type: 'COMMAND',
      unit_path: 'week1/unit',
      session_key: 'check',
      command: 'cargo test',
    }),
    {
      type: 'COMMAND',
      unit_path: 'week1/unit',
      session_key: 'check',
      cwd: '.',
      command: 'cargo test',
      accepted_exit_codes: [0],
      expected_output: [],
      timeout_ms: 300_000,
      save_dirty: false,
    },
  )
})

test('rejects invalid check fields', () => {
  assert.equal(
    parseCheckPayload({
      type: 'COMMAND',
      unit_path: 'week1/unit',
      session_key: 'check',
      command: 'cargo test',
      timeout_ms: 0,
    }),
    undefined,
  )
})

test('lets an Adapter validate its Unit path', () => {
  const rejectAll = () => undefined
  assert.equal(
    parseCheckPayload(
      {
        type: 'COMMAND',
        unit_path: 'bad/../unit',
        session_key: 'check',
        command: 'cargo test',
      },
      { normalizeUnitPath: rejectAll },
    ),
    undefined,
  )
})
