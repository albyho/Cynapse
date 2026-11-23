import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  parseTerminalCommandOutput,
  wrapTerminalCommand,
} from './terminalCommand'

test('the PowerShell wrapper reports the exit code after a begin marker', () => {
  const wrapped = wrapTerminalCommand(
    'powershell',
    'python -m pytest; python check.py',
  )

  assert.ok(wrapped.includes("Write-Output '__CYNAPSE_BEGIN__'"))
  assert.ok(wrapped.includes('& { python -m pytest; python check.py }'))
  assert.ok(wrapped.includes('__CYNAPSE_EXIT__:$cynapseCode'))
  assert.ok(wrapped.includes('$LASTEXITCODE'))
})

test('the POSIX wrapper reports the exit status after a begin marker', () => {
  const wrapped = wrapTerminalCommand('posix', 'python -m pytest')

  assert.ok(wrapped.includes("printf '\\n__CYNAPSE_BEGIN__\\n'"))
  assert.ok(wrapped.includes('python -m pytest; cynapse_code=$?'))
  assert.ok(wrapped.includes('__CYNAPSE_EXIT__:%s'))
})

test('parses command output from a terminal echo and prompt', () => {
  const raw =
    'PS C:\\workspace> Write-Output \'__CYNAPSE_BEGIN__\'; & { python --version }; $cynapseOk = $?; Write-Output "`n__CYNAPSE_EXIT__:$cynapseCode"\r\n' + // test-isolation-allow: terminal echo fixture
    '__CYNAPSE_BEGIN__\r\n' +
    '\u001b[32mPython 3.13.0\u001b[0m\r\n' +
    '\r\n__CYNAPSE_EXIT__:0\r\n' +
    'PS C:\\workspace> ' // test-isolation-allow: terminal prompt fixture

  assert.deepEqual(parseTerminalCommandOutput(raw), {
    output: 'Python 3.13.0\n',
    exitCode: 0,
    markerFound: true,
  })
})

test('keeps output without a trailing newline and reports a non-zero exit', () => {
  const raw =
    'sh-5.2$ printf ...\n' +
    '__CYNAPSE_BEGIN__\n' +
    'AssertionError\n' +
    '__CYNAPSE_EXIT__:2\n'

  assert.deepEqual(parseTerminalCommandOutput(raw), {
    output: 'AssertionError',
    exitCode: 2,
    markerFound: true,
  })
})

test('a partial exit marker without its newline is not parsed', () => {
  const raw = '__CYNAPSE_BEGIN__\noutput\n__CYNAPSE_EXIT__:1'

  assert.deepEqual(parseTerminalCommandOutput(raw), {
    output: 'output\n__CYNAPSE_EXIT__:1',
    exitCode: undefined,
    markerFound: false,
  })
})

test('an interrupted command has no exit marker and keeps the partial output', () => {
  const raw = '__CYNAPSE_BEGIN__\npartial output\n'

  assert.deepEqual(parseTerminalCommandOutput(raw), {
    output: 'partial output\n',
    exitCode: undefined,
    markerFound: false,
  })
})
