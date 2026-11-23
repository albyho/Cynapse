import assert from 'node:assert/strict'
import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'
import { afterEach, beforeEach, test } from 'node:test'

import {
  globalRuntimePath,
  parseDiscovery,
  resolveDiscovery,
  type Discovery,
} from './discovery'

let tempRoot: string | undefined
let originalLocalAppData: string | undefined

beforeEach(() => {
  originalLocalAppData = process.env.LOCALAPPDATA
  tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'cynapse-discovery-'))
  process.env.LOCALAPPDATA = tempRoot
})

afterEach(() => {
  if (originalLocalAppData === undefined) {
    delete process.env.LOCALAPPDATA
  } else {
    process.env.LOCALAPPDATA = originalLocalAppData
  }
  if (tempRoot) {
    fs.rmSync(tempRoot, { recursive: true, force: true })
  }
})

// Out of range for real PIDs, so process.kill cannot report EPERM for it.
const DEAD_PID = 2 ** 31 - 1

function runtimeFilePath(): string {
  const clientDirectory = path.join(tempRoot!, 'Cynapse', 'Client')
  fs.mkdirSync(clientDirectory, { recursive: true })
  return path.join(clientDirectory, 'runtime.json')
}

function writeDiscovery(overrides: Partial<Discovery> = {}): string {
  const runtimePath = runtimeFilePath()
  fs.writeFileSync(
    runtimePath,
    JSON.stringify({
      workspace: 'course-workspace',
      port: 51234,
      token: 'test-token',
      pid: process.pid,
      started_at: 1,
      ...overrides,
    }),
  )
  return runtimePath
}

test('resolveDiscovery reads the Client runtime file from its product directory', () => {
  const runtimePath = writeDiscovery()

  assert.equal(globalRuntimePath(), runtimePath)
  assert.equal(resolveDiscovery()?.port, 51234)
  assert.equal(fs.existsSync(runtimePath), true)
})

test('resolveDiscovery leaves a stale runtime file in place', () => {
  const runtimePath = writeDiscovery({
    token: 'stale-token',
    pid: DEAD_PID,
  })

  assert.equal(resolveDiscovery(), undefined)
  assert.equal(fs.existsSync(runtimePath), true)
})

test('resolveDiscovery reads a replacement runtime file after a stale one', () => {
  writeDiscovery({
    port: 51233,
    token: 'stale-token',
    pid: DEAD_PID,
  })
  assert.equal(resolveDiscovery(), undefined)

  writeDiscovery({
    port: 51234,
    token: 'fresh-token',
    started_at: 2,
  })

  assert.equal(resolveDiscovery()?.port, 51234)
})

test('resolveDiscovery leaves a malformed runtime file in place', () => {
  const runtimePath = runtimeFilePath()
  fs.writeFileSync(runtimePath, '{not json')

  assert.equal(resolveDiscovery(), undefined)
  assert.equal(fs.existsSync(runtimePath), true)
})

test('the shared runtime discovery fixture matches the contract', () => {
  const raw = fs.readFileSync(
    path.resolve(
      __dirname,
      '../../../protocol/contracts/runtime-discovery.json',
    ),
    'utf8',
  )
  const discovery = parseDiscovery(raw)

  assert.equal(discovery?.port, 49152)
  assert.equal(typeof discovery?.workspace, 'string')
  assert.equal(discovery?.pid, 4242)
})
