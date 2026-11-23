import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

import Ajv2020 from 'ajv/dist/2020'
import addFormats from 'ajv-formats'

const protocolRoot = path.resolve(__dirname, '../../../protocol')
const schema = JSON.parse(
  readFileSync(path.join(protocolRoot, 'schema.json'), 'utf8'),
) as object

function collectJsonFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name)
    return entry.isDirectory()
      ? collectJsonFiles(entryPath)
      : entry.name.endsWith('.json')
        ? [entryPath]
        : []
  })
}

function validator() {
  const ajv = new Ajv2020({ allErrors: true, strict: false })
  addFormats(ajv)
  return ajv.compile(schema)
}

test('all protocol golden fixtures match the shared schema', () => {
  const validate = validator()
  for (const fixtureFile of collectJsonFiles(
    path.join(protocolRoot, 'test-data'),
  )) {
    const fixture = JSON.parse(readFileSync(fixtureFile, 'utf8'))
    assert.equal(validate(fixture), true, fixtureFile)
  }
})

test('all invalid protocol fixtures are rejected by the shared schema', () => {
  const validate = validator()
  for (const fixtureFile of collectJsonFiles(
    path.join(protocolRoot, 'invalid-test-data'),
  )) {
    const fixture = JSON.parse(readFileSync(fixtureFile, 'utf8'))
    assert.equal(validate(fixture), false, fixtureFile)
  }
})
