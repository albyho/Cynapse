import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'

import { CELL_SCROLL_ALIGNS } from './protocol'

interface CourseNode {
  children?: CourseNode[]
  steps?: { step_file: string }[]
}

interface CourseAction {
  type: string
  align?: string
  cell_id?: string
  from_cell_id?: string
  to_cell_id?: string
  after_cell_id?: string
  new_cell_id?: string
  cell_type?: 'code' | 'markdown'
  source?: string
  speed_ms?: number
}

const fixture = path.join(process.cwd(), 'test-data', 'jupyterlab-operations')

function stepFiles(node: CourseNode): string[] {
  return [
    ...(node.steps ?? []).map((step) => step.step_file),
    ...(node.children ?? []).flatMap(stepFiles),
  ]
}

test('the operations fixture covers every JupyterLab Adapter action', () => {
  const manifest = JSON.parse(
    readFileSync(path.join(fixture, 'manifest.json'), 'utf8'),
  ) as CourseNode
  const files = stepFiles(manifest)
  assert.ok(files.length > 0)
  for (const seed of [
    'seeds/ch/notebook_ops/INTRO.md',
    'seeds/ch/notebook_ops/lesson.ipynb',
    'seeds/ch/scroll_align/scroll_align.ipynb',
    'seeds/ch/empty_notebook/empty.ipynb',
  ]) {
    assert.ok(existsSync(path.join(fixture, seed)), `missing ${seed}`)
  }

  const actions: CourseAction[] = []
  for (const file of files) {
    const stepFile = path.join(fixture, file)
    assert.ok(existsSync(stepFile), `missing ${file}`)
    const step = JSON.parse(readFileSync(stepFile, 'utf8')) as {
      actions: CourseAction[]
    }
    actions.push(...step.actions)
  }

  assert.ok(
    actions.some(
      (action) =>
        action.type === 'JupyterLabCellInsert' &&
        action.after_cell_id === undefined,
    ),
    'fixture does not cover first-Cell insertion',
  )

  const types = new Set(actions.map((action) => action.type))
  for (const type of [
    'JupyterLabCloseDocument',
    'JupyterLabInterruptKernel',
    'JupyterLabOpenNotebook',
    'JupyterLabOpenPreview',
    'JupyterLabRestartKernel',
    'JupyterLabCellDelete',
    'JupyterLabCellDuplicate',
    'JupyterLabCellInsert',
    'JupyterLabCellInsertText',
    'JupyterLabCellMove',
    'JupyterLabCellRun',
    'JupyterLabCellSelect',
    'TerminalEnsure',
    'TerminalCommand',
  ]) {
    assert.ok(types.has(type), `fixture does not cover ${type}`)
  }

  const aligns = new Set(
    actions
      .filter((action) => action.type === 'JupyterLabCellSelect')
      .map((action) => action.align),
  )
  for (const align of CELL_SCROLL_ALIGNS) {
    assert.ok(aligns.has(align), `fixture does not cover align ${align}`)
  }

  const inserted = new Set<string>()
  const codeInserted = new Set<string>()
  const duplicated = new Set<string>()
  const typed = new Set<string>()
  const run = new Set<string>()
  let allRun = false
  let rangeRun = false
  for (const action of actions) {
    if (action.type === 'JupyterLabCellInsert') {
      assert.ok(action.cell_id)
      assert.ok(!inserted.has(action.cell_id), `duplicate ${action.cell_id}`)
      inserted.add(action.cell_id)
      if (action.cell_type === 'code') {
        assert.equal(action.source, '')
        codeInserted.add(action.cell_id)
      }
    }
    if (action.type === 'JupyterLabCellInsertText') {
      assert.ok(action.cell_id)
      assert.ok(action.speed_ms !== undefined && action.speed_ms > 0)
      typed.add(action.cell_id)
    }
    if (action.type === 'JupyterLabCellDuplicate') {
      assert.ok(action.cell_id)
      assert.ok(action.new_cell_id)
      assert.ok(
        !inserted.has(action.new_cell_id) &&
          !duplicated.has(action.new_cell_id),
        `duplicate target ${action.new_cell_id} already exists`,
      )
      duplicated.add(action.new_cell_id)
    }
    if (action.type === 'JupyterLabCellRun') {
      if (action.from_cell_id && action.to_cell_id) {
        if (action.from_cell_id !== action.to_cell_id) {
          rangeRun = true
        }
        run.add(action.to_cell_id)
      } else if (!action.from_cell_id && !action.to_cell_id) {
        allRun = true
      } else if (action.from_cell_id) {
        run.add(action.from_cell_id)
      }
    }
    if (action.type === 'JupyterLabCellDelete') {
      assert.ok(action.cell_id)
      assert.ok(
        inserted.has(action.cell_id) || duplicated.has(action.cell_id),
        `deleting seed ${action.cell_id}`,
      )
    }
  }

  for (const cellId of codeInserted) {
    assert.ok(typed.has(cellId), `inserted ${cellId} was never typed`)
    assert.ok(
      run.has(cellId) || allRun,
      `inserted ${cellId} was never executed`,
    )
  }
  assert.ok(rangeRun, 'fixture does not cover a Cell range')
})
