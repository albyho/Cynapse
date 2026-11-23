import assert from 'node:assert/strict'
import { test } from 'node:test'

import { suffixPrefixLength } from './textMatching'

test('returns the longest expected prefix that ends the actual text', () => {
  assert.equal(suffixPrefixLength('abc', 'abcd'), 3)
  assert.equal(suffixPrefixLength('abcx', 'abcd'), 0)
  assert.equal(suffixPrefixLength('xabc', 'abc'), 3)
  assert.equal(suffixPrefixLength('校验', '校验x'), '校验'.length)
  assert.equal(suffixPrefixLength('', 'abcd'), 0)
  assert.equal(suffixPrefixLength('abc', ''), 0)
})
