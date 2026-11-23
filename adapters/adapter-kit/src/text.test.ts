import assert from 'node:assert/strict'
import { test } from 'node:test'

import { splitIntoTypewriterChunks, typewriterTokens } from './text'

test('typewriter groups a four-space indentation as one token', () => {
  assert.deepEqual(typewriterTokens('    x\n  y'), [
    '    ',
    'x',
    '\n',
    ' ',
    ' ',
    'y',
  ])
})

test('typewriter chunks preserve the source bytes', () => {
  const source = '  const value = 1\n'
  const chunks = splitIntoTypewriterChunks(source, 100)
  assert.equal(chunks.map((chunk) => chunk.text).join(''), source)
  assert.ok(chunks.length > 1)
})

test('speed zero keeps the source in one chunk', () => {
  const source = '  const value = 1\n'
  assert.deepEqual(splitIntoTypewriterChunks(source, 0), [
    { text: source, units: typewriterTokens(source).length },
  ])
})

test('typewriter splits eight leading spaces into groups of four', () => {
  assert.deepEqual(typewriterTokens('  ok\n        done\n'), [
    ' ',
    ' ',
    'o',
    'k',
    '\n',
    '    ',
    '    ',
    'd',
    'o',
    'n',
    'e',
    '\n',
  ])
})

test('typewriter keeps inline spaces as individual characters', () => {
  assert.deepEqual(typewriterTokens('a    b'), ['a', ' ', ' ', ' ', ' ', 'b'])
})

test('typewriter leaves mid-line indentation ungrouped', () => {
  assert.deepEqual(typewriterTokens('    let x = 1', false), [
    ' ',
    ' ',
    ' ',
    ' ',
    'l',
    'e',
    't',
    ' ',
    'x',
    ' ',
    '=',
    ' ',
    '1',
  ])
  assert.deepEqual(splitIntoTypewriterChunks('    x', 80, false), [
    { text: ' ', units: 1 },
    { text: ' ', units: 1 },
    { text: ' ', units: 1 },
    { text: ' ', units: 1 },
    { text: 'x', units: 1 },
  ])
})

test('chunks at 80ms are one token each', () => {
  assert.deepEqual(splitIntoTypewriterChunks('    x\n', 80), [
    { text: '    ', units: 1 },
    { text: 'x', units: 1 },
    { text: '\n', units: 1 },
  ])
})
