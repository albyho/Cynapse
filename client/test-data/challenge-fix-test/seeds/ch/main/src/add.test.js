import test from 'node:test'
import assert from 'node:assert/strict'

import { add } from './add.js'

test('adds two numbers', () => {
  assert.strictEqual(add(2, 3), 5)
})
