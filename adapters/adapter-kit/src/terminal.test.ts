import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  MAX_TERMINAL_OUTPUT_BYTES,
  TERMINAL_TRUNCATION_MARKER,
  boundTerminalOutput,
  classifyTerminalResult,
  normalizeTerminalOutput,
} from './terminal'

test('normalizes ANSI escapes and line endings', () => {
  assert.equal(
    normalizeTerminalOutput('\u001b[31mred\u001b[0m\r\nnext\rline'),
    'red\nnext\nline',
  )
})

test('keeps output within the byte budget', () => {
  const result = boundTerminalOutput('hello\n')
  assert.deepEqual(result, { output: 'hello\n', truncated: false })
})

test('keeps the UTF-8 tail and adds the truncation marker', () => {
  const result = boundTerminalOutput(
    'a'.repeat(MAX_TERMINAL_OUTPUT_BYTES) + '你',
  )
  assert.equal(result.truncated, true)
  assert.ok(result.output.startsWith(TERMINAL_TRUNCATION_MARKER))
  assert.ok(result.output.endsWith('你'))
  assert.ok(!result.output.includes('\uFFFD'))
})

test('an already-truncated result remains marked', () => {
  const result = boundTerminalOutput('short', true)
  assert.equal(result.truncated, true)
  assert.equal(result.output, TERMINAL_TRUNCATION_MARKER + 'short')
})

test('classifies terminal success and expected-output mismatch', () => {
  const done = classifyTerminalResult(
    {
      exitCode: 0,
      output: 'ok\r\n',
      cancelled: false,
      timedOut: false,
    },
    { acceptedExitCodes: [0], expectedOutput: ['ok'] },
  )
  assert.equal(done.kind, 'done')

  const mismatch = classifyTerminalResult(
    {
      exitCode: 0,
      output: 'no',
      cancelled: false,
      timedOut: false,
    },
    { acceptedExitCodes: [0], expectedOutput: ['ok'] },
  )
  assert.equal(mismatch.kind, 'output_mismatch')
  assert.deepEqual(mismatch.missingExpectedOutput, ['ok'])
})

test('classifies timeouts before exit-code failure', () => {
  const timeout = classifyTerminalResult(
    {
      exitCode: undefined,
      output: 'partial',
      cancelled: false,
      timedOut: true,
    },
    { acceptedExitCodes: [0], expectedOutput: [] },
  )
  assert.equal(timeout.kind, 'timed_out')
})
