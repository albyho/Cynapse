import assert from 'node:assert/strict'
import { test } from 'node:test'
import { runChunkedEdit } from './chunkedEdit'
import type { TypewriterChunk } from '@cynapse/adapter-kit'
import { FakeFacade } from './testSupport'

const chunks: TypewriterChunk[] = [
  { text: 'a', units: 1 },
  { text: 'b', units: 1 },
  { text: 'c', units: 1 },
]

function makePlan(facade: FakeFacade, applied: string[], restore: () => void) {
  return {
    target: facade.target,
    chunks,
    speedMs: 1,
    applyChunk: async (chunk: TypewriterChunk) => {
      applied.push(chunk.text)
    },
    restore: async () => {
      restore()
    },
  }
}

test('applies every chunk and saves once', async () => {
  const facade = new FakeFacade()
  const applied: string[] = []
  let restores = 0

  const outcome = await runChunkedEdit(
    makePlan(facade, applied, () => {
      restores += 1
    }),
    facade,
  )

  assert.equal(outcome, 'done')
  assert.deepEqual(applied, ['a', 'b', 'c'])
  assert.equal(facade.saves, 1)
  assert.equal(restores, 0)
})

for (const abortAt of [1, 2, 3]) {
  test(`restores after abort following chunk ${abortAt}`, async () => {
    const facade = new FakeFacade()
    const controller = new AbortController()
    const applied: string[] = []
    let restores = 0
    const plan = makePlan(facade, applied, () => {
      restores += 1
    })
    plan.applyChunk = async (chunk: TypewriterChunk) => {
      applied.push(chunk.text)
      if (applied.length === abortAt) {
        controller.abort()
      }
    }

    const outcome = await runChunkedEdit(plan, facade, controller.signal)

    assert.equal(outcome, 'cancelled')
    assert.deepEqual(
      applied,
      chunks.slice(0, abortAt).map((chunk) => chunk.text),
    )
    assert.equal(facade.saves, 0)
    assert.equal(restores, 1)
  })
}

test('restores when abort arrives while saving', async () => {
  const facade = new FakeFacade()
  const controller = new AbortController()
  const applied: string[] = []
  let restores = 0
  facade.save = async () => {
    facade.saved = true
    facade.saves += 1
    controller.abort()
  }
  const plan = makePlan(facade, applied, () => {
    restores += 1
  })

  const outcome = await runChunkedEdit(plan, facade, controller.signal)

  assert.equal(outcome, 'cancelled')
  assert.deepEqual(applied, ['a', 'b', 'c'])
  assert.equal(facade.saves, 1)
  assert.equal(restores, 1)
})

test('does not save or restore when preparation is cancelled', async () => {
  const facade = new FakeFacade()
  const applied: string[] = []
  let restores = 0
  const controller = new AbortController()
  const plan = {
    ...makePlan(facade, applied, () => {
      restores += 1
    }),
    prepare: async () => {
      controller.abort()
      return false
    },
  }

  const outcome = await runChunkedEdit(plan, facade, controller.signal)

  assert.equal(outcome, 'cancelled')
  assert.deepEqual(applied, [])
  assert.equal(facade.saves, 0)
  assert.equal(restores, 0)
})
