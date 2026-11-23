import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

import { parseCourseAction } from './actions'
import {
  FakeFacade,
  applyOpenedEdit,
  type OpenedEditAction,
} from './testSupport'

interface SharedCase {
  name: string
  text: string
  action: unknown
  expected: { text: string }
}

function isEditAction(action: { type: string }): action is OpenedEditAction {
  return (
    action.type === 'InsertText' ||
    action.type === 'DeleteText' ||
    action.type === 'ReplaceText'
  )
}

test('matches the shared text edit contract', async () => {
  const fixture = JSON.parse(
    readFileSync(
      path.resolve(
        __dirname,
        '../../../protocol/contracts/text-edit-cases.json',
      ),
      'utf8',
    ),
  ) as { cases: SharedCase[] }

  for (const testCase of fixture.cases) {
    const action = parseCourseAction(testCase.action)
    if (!action || !isEditAction(action)) {
      throw new Error(`${testCase.name}: expected a code edit action`)
    }
    const facade = new FakeFacade()
    facade.existingText = testCase.text
    facade.target.lineCount = testCase.text.split('\n').length
    await applyOpenedEdit(action, facade)
    assert.equal(facade.existingText, testCase.expected.text, testCase.name)
  }
})
