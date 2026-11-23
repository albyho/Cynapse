import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  JupyterLabFacadeError,
  MAX_CELL_ERROR_CHARS,
  MAX_CELL_OUTPUT_CHARS,
  MAX_CELL_OUTPUTS,
  MAX_CELL_TRACEBACK_LINES,
  applyJupyterLabAction,
  boundedOutput,
  createJupyterLabActionHandler,
  type CellExecution,
  type JupyterLabFacade,
} from './actions'
import type {
  CellDeleteTarget,
  CellEditRange,
  CellInsertTarget,
} from './cellTextEdit'
import type {
  AckPayload,
  CellScrollAlign,
  Envelope,
  JupyterLabAction,
} from './protocol'
import { parseJupyterLabAction } from './protocol'
import type { TerminalFacade } from './terminal'

class FakeJupyterLabFacade implements JupyterLabFacade {
  readonly calls: string[] = []
  readonly failures = new Map<string, JupyterLabFacadeError>()
  execution: CellExecution = {
    outputs: [],
    text: '',
  }
  runGate: Promise<void> | undefined
  insertTextGate: Promise<void> | undefined

  async openNotebook(path: string): Promise<void> {
    this.calls.push(`open:${path}`)
    this.fail('openNotebook')
  }

  async selectCell(
    path: string,
    cellId: string,
    scroll: boolean,
    align: CellScrollAlign,
  ): Promise<void> {
    this.calls.push(`select:${path}:${cellId}:${scroll}:${align}`)
    this.fail('selectCell')
  }

  async openPreview(path: string): Promise<void> {
    this.calls.push(`preview:${path}`)
    this.fail('openPreview')
  }

  async closeDocument(path: string): Promise<void> {
    this.calls.push(`close:${path}`)
    this.fail('closeDocument')
  }

  async insertText(
    path: string,
    cellId: string,
    target: CellInsertTarget,
    text: string,
    speedMs: number,
    targetHoldMs: number | undefined,
  ): Promise<void> {
    this.calls.push(
      `insert:${path}:${cellId}:${JSON.stringify(target)}:${text}:${speedMs}:${targetHoldMs ?? '-'}`,
    )
    await this.insertTextGate
    this.fail('insertText')
  }

  async deleteText(
    path: string,
    cellId: string,
    target: CellDeleteTarget,
    direction: 'backspace' | 'delete',
    speedMs: number,
    targetHoldMs: number | undefined,
  ): Promise<void> {
    this.calls.push(
      `delete-text:${path}:${cellId}:${JSON.stringify(target)}:${direction}:${speedMs}:${targetHoldMs ?? '-'}`,
    )
    this.fail('deleteText')
  }

  async replaceText(
    path: string,
    cellId: string,
    target: CellEditRange,
    text: string,
    speedMs: number,
    targetHoldMs: number | undefined,
  ): Promise<void> {
    this.calls.push(
      `replace-text:${path}:${cellId}:${JSON.stringify(target)}:${text}:${speedMs}:${targetHoldMs ?? '-'}`,
    )
    this.fail('replaceText')
  }

  async insertCell(
    path: string,
    cellId: string,
    afterCellId: string | null,
    beforeCellId: string | null,
    cellType: 'code' | 'markdown',
    source: string,
  ): Promise<void> {
    this.calls.push(
      `insert:${path}:${cellId}:${beforeCellId ?? '-'}:${afterCellId ?? 'end'}:${cellType}:${source}`,
    )
    this.fail('insertCell')
  }

  async moveCell(
    path: string,
    cellId: string,
    direction: 'up' | 'down',
    count: number,
  ): Promise<void> {
    this.calls.push(`move:${path}:${cellId}:${direction}:${count}`)
    this.fail('moveCell')
  }

  async duplicateCell(
    path: string,
    cellId: string,
    newCellId: string | null,
  ): Promise<void> {
    this.calls.push(`duplicate:${path}:${cellId}:${newCellId ?? '-'}`)
    this.fail('duplicateCell')
  }

  async deleteCell(path: string, cellId: string): Promise<void> {
    this.calls.push(`delete:${path}:${cellId}`)
    this.fail('deleteCell')
  }

  async saveNotebook(path: string): Promise<void> {
    this.calls.push(`save:${path}`)
    this.fail('saveNotebook')
  }

  async runCell(
    path: string,
    fromCellId: string | null,
    toCellId: string | null,
    timeoutMs: number,
    signal: AbortSignal,
  ): Promise<CellExecution> {
    this.calls.push(
      `run:${path}:${fromCellId ?? '-'}:${toCellId ?? '-'}:${timeoutMs}`,
    )
    await this.runGate
    if (signal.aborted) {
      throw new Error('aborted')
    }
    this.fail('runCell')
    return this.execution
  }

  async restartKernel(path: string): Promise<void> {
    this.calls.push(`restart:${path}`)
    this.fail('restartKernel')
  }

  async interruptKernel(path: string): Promise<void> {
    this.calls.push(`interrupt:${path}`)
    this.fail('interruptKernel')
  }

  private fail(method: keyof JupyterLabFacade): void {
    const failure = this.failures.get(method)
    if (failure) {
      throw failure
    }
  }
}

function envelope(payload: Record<string, unknown>): Envelope {
  return {
    v: 2,
    id: '00000000-0000-4000-8000-000000000001',
    type: 'action',
    payload,
  }
}

function context(signal = new AbortController().signal) {
  return { signal }
}

function selector(cellId = 'cell-1'): Record<string, unknown> {
  return {
    type: 'JupyterLabCellSelect',
    path: 'lesson.ipynb',
    cell_id: cellId,
  }
}

test('JupyterLabCellSelect defaults to scrolling to the Cell', () => {
  assert.deepEqual(parseJupyterLabAction(selector()), {
    type: 'JupyterLabCellSelect',
    path: 'lesson.ipynb',
    cell_id: 'cell-1',
    scroll: true,
    align: 'auto',
  })
})

test('JupyterLabCellInsertText accepts direct mode and JupyterLabCellRun defaults its timeout', () => {
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'x = 1\n',
      speed_ms: 0,
    }),
    {
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'x = 1\n',
      speed_ms: 0,
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'x = 1\n',
      speed_ms: null,
    }),
    {
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'x = 1\n',
      speed_ms: 0,
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'setup',
      to_cell_id: 'cell-1',
    }),
    {
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'setup',
      to_cell_id: 'cell-1',
      timeout_ms: 300_000,
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
    }),
    {
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      timeout_ms: 300_000,
    },
  )
})

test('parses preview, typewriter, insert, and delete actions', () => {
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabOpenPreview',
      path: 'INTRO.md',
    }),
    { type: 'JupyterLabOpenPreview', path: 'INTRO.md' },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCloseDocument',
      path: 'INTRO.md',
    }),
    { type: 'JupyterLabCloseDocument', path: 'INTRO.md' },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'x = 1\n',
    }),
    {
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'x = 1\n',
      speed_ms: 0,
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellInsert',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
      after_cell_id: 'cell-1',
      cell_type: 'code',
      source: 'x = 1\n',
    }),
    {
      type: 'JupyterLabCellInsert',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
      after_cell_id: 'cell-1',
      cell_type: 'code',
      source: 'x = 1\n',
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellInsert',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_first',
      cell_type: 'code',
      source: '',
    }),
    {
      type: 'JupyterLabCellInsert',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_first',
      cell_type: 'code',
      source: '',
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellMove',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
      direction: 'up',
    }),
    {
      type: 'JupyterLabCellMove',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
      direction: 'up',
      count: 1,
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellDuplicate',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
    }),
    {
      type: 'JupyterLabCellDuplicate',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
    },
  )
  assert.deepEqual(
    parseJupyterLabAction({
      type: 'JupyterLabCellDelete',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
    }),
    {
      type: 'JupyterLabCellDelete',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
    },
  )
})

test('accepts every text range placement pair and rejects invalid ranges', () => {
  const range = (
    start: Record<string, unknown>,
    end: Record<string, unknown>,
  ): Record<string, unknown> => ({
    start,
    end,
    expected_text: 'old',
  })
  const action = (
    target: Record<string, unknown>,
  ): Record<string, unknown> => ({
    type: 'JupyterLabCellReplaceText',
    path: 'lesson.ipynb',
    cell_id: 'cell-1',
    target,
    text: 'replacement',
    speed_ms: 0,
  })

  const validText = [
    [{ at: { text: 'old' } }, { at: { text: 'new' } }],
    [
      { at: { text: 'old' }, placement: 'before' },
      { at: { text: 'new' }, placement: 'after' },
    ],
    [
      { at: { text: 'old' }, placement: 'after' },
      { at: { text: 'new' }, placement: 'before' },
    ],
    [
      { at: { text: 'old' }, placement: 'before' },
      { at: { text: 'new' }, placement: 'before' },
    ],
    [
      { at: { text: 'old' }, placement: 'after' },
      { at: { text: 'new' }, placement: 'after' },
    ],
    [
      { line: 2, at: { text: 'old', index: 1 } },
      { line: 2, at: { text: 'new' }, placement: 'after' },
    ],
  ]
  for (const [start, end] of validText) {
    assert.notEqual(parseJupyterLabAction(action(range(start, end))), undefined)
  }

  const validPoint = [
    [
      { line: 1, at: 0 },
      { line: 1, at: 3 },
    ],
    [{ line: 1 }, { line: 2 }],
    [{ line: 1, at: 0 }, { line: 1 }],
    [
      { line: 1, at: 1 },
      { line: 1, at: 1 },
    ],
    [
      { line: 1, at: { text: 'old' } },
      { line: 2, at: 0 },
    ],
    [{ line: 1 }, { eof: true }],
  ]
  for (const [start, end] of validPoint) {
    assert.notEqual(parseJupyterLabAction(action(range(start, end))), undefined)
  }

  const invalid = [
    [
      { at: { text: 'old' }, placement: 'after' },
      { at: { text: 'old', index: 0 }, placement: 'before' },
    ],
    [
      { line: 1, at: { text: 'old' } },
      { line: 1, at: { text: 'old' }, placement: 'before' },
    ],
    [{ eof: true }, { line: 2 }],
    [{ line: 2 }, { line: 1 }],
    [
      { line: 1, at: 3 },
      { line: 1, at: 2 },
    ],
    [{ line: 1 }, { eof: true, line: 2 }],
  ]
  for (const [start, end] of invalid) {
    assert.equal(parseJupyterLabAction(action(range(start, end))), undefined)
  }
})

test('enforces the Anchor validation matrix and rejects old target shapes', () => {
  const insert = (target: unknown): Record<string, unknown> => ({
    type: 'JupyterLabCellInsertText',
    path: 'lesson.ipynb',
    cell_id: 'cell-1',
    target,
    text: 'x',
  })
  const valid = [
    { eof: true },
    { line: 1 },
    { line: 1, placement: 'after' },
    { line: 1, at: 0 },
    { line: 1, at: 4, placement: 'after' },
    { at: { text: 'x' } },
    { at: { text: 'x', index: 2 } },
    { line: 2, at: { text: 'x', index: 0 }, placement: 'before' },
  ]
  for (const target of valid) {
    assert.notEqual(
      parseJupyterLabAction(insert(target)),
      undefined,
      JSON.stringify(target),
    )
  }
  assert.deepEqual(
    parseJupyterLabAction(insert({ line: 2, at: { text: 'x', index: 1 } })),
    {
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { line: 2, at: { text: 'x', index: 1 } },
      text: 'x',
      speed_ms: 0,
    },
  )

  const invalid = [
    {},
    { line: 0 },
    { line: 1.5 },
    { line: '1' },
    { line: null },
    { at: 0 },
    { at: null },
    { line: 1, at: null },
    { line: 1, at: -1 },
    { line: 1, at: 1.5 },
    { line: 1, at: { text: '' } },
    { line: 1, at: { text: 'x', index: null } },
    { line: 1, at: { text: 'x', index: -1 } },
    { line: 1, at: { text: 'x', index: 1.5 } },
    { line: 1, at: { text: 'x', extra: true } },
    { line: 1, at: { text: 1 } },
    { line: 1, placement: 'inside' },
    { line: 1, placement: null },
    { eof: true, line: 1 },
    { eof: true, at: 0 },
    { eof: true, placement: 'before' },
    { eof: null },
    { eof: 'yes' },
    { line: 1, extra: true },
    { kind: 'end_of_file' },
    { kind: 'line', line: 1 },
    { kind: 'position', line: 1, column: 0 },
    { kind: 'literal', text: 'x', placement: 'before' },
    { line: 1, column: 0 },
    { text: 'x' },
  ]
  for (const target of invalid) {
    assert.equal(
      parseJupyterLabAction(insert(target)),
      undefined,
      JSON.stringify(target),
    )
  }

  const range = (target: Record<string, unknown>): Record<string, unknown> => ({
    type: 'JupyterLabCellDeleteText',
    path: 'lesson.ipynb',
    cell_id: 'cell-1',
    target,
  })
  assert.notEqual(
    parseJupyterLabAction(
      range({ start: { line: 1 }, end: { eof: true }, expected_text: 'x' }),
    ),
    undefined,
  )
  for (const target of [
    {
      start: { line: 1 },
      end: { line: 2 },
      expected_text: 'x',
      include_end_line_break: true,
    },
    {
      start: { line: 1, column: 0 },
      end: { line: 2 },
      expected_text: 'x',
    },
    {
      start: { line: 1 },
      end: { line: 2 },
      expected_text: '',
    },
    {
      start: { line: 1 },
      end: { line: 2 },
      expected_text: null,
    },
    {
      start: { text: 'old' },
      end: { text: 'new' },
      expected_text: 'old',
    },
  ]) {
    assert.equal(
      parseJupyterLabAction(range(target)),
      undefined,
      JSON.stringify(target),
    )
  }
})

test('dispatches preview, typewriter, insert, and delete actions', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  assert.deepEqual(
    await handler(
      envelope({ type: 'JupyterLabOpenPreview', path: 'INTRO.md' }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({ type: 'JupyterLabCloseDocument', path: 'INTRO.md' }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({
        type: 'JupyterLabCellInsertText',
        path: 'lesson.ipynb',
        cell_id: 'cell-1',
        target: { eof: true },
        text: 'x = 1\n',
        speed_ms: 5,
      }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({
        type: 'JupyterLabCellInsert',
        path: 'lesson.ipynb',
        cell_id: 'cynapse_demo_1',
        after_cell_id: 'cell-1',
        cell_type: 'code',
        source: 'x = 1\n',
      }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({
        type: 'JupyterLabCellMove',
        path: 'lesson.ipynb',
        cell_id: 'cynapse_demo_1',
        direction: 'up',
      }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({
        type: 'JupyterLabCellDuplicate',
        path: 'lesson.ipynb',
        cell_id: 'cynapse_demo_1',
        new_cell_id: 'cynapse_demo_1_copy',
      }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({
        type: 'JupyterLabCellDelete',
        path: 'lesson.ipynb',
        cell_id: 'cynapse_demo_1',
      }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({ type: 'JupyterLabCloseDocument', path: 'lesson.ipynb' }),
      context(),
    ),
    { status: 'done' },
  )

  assert.deepEqual(facade.calls, [
    'preview:INTRO.md',
    'close:INTRO.md',
    'insert:lesson.ipynb:cell-1:{"eof":true}:x = 1\n:5:-',
    'save:lesson.ipynb',
    'insert:lesson.ipynb:cynapse_demo_1:-:cell-1:code:x = 1\n',
    'save:lesson.ipynb',
    'move:lesson.ipynb:cynapse_demo_1:up:1',
    'save:lesson.ipynb',
    'duplicate:lesson.ipynb:cynapse_demo_1:cynapse_demo_1_copy',
    'save:lesson.ipynb',
    'delete:lesson.ipynb:cynapse_demo_1',
    'save:lesson.ipynb',
    'close:lesson.ipynb',
  ])
})

test('Present is a successful no-op for the JupyterLab Adapter', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  assert.deepEqual(await handler(envelope({ type: 'Present' }), context()), {
    status: 'done',
  })
  assert.deepEqual(facade.calls, [])
})

test('the parser rejects missing Cell ids and actions owned by other Adapters', () => {
  const invalid = [
    { type: 'JupyterLabCellSelect', path: 'lesson.ipynb' },
    { type: 'JupyterLabCellSelect', path: 'lesson.ipynb', cell_id: '' },
    {
      type: 'JupyterLabCellSelect',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      align: 'middle',
    },
    {
      type: 'JupyterLabCellInsert',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
      after_cell_id: 'cell-1',
      cell_type: 'raw',
      source: '',
    },
    {
      type: 'JupyterLabCellInsert',
      path: 'lesson.ipynb',
      cell_id: 'cynapse_demo_1',
      after_cell_id: 'cell-1',
      before_cell_id: 'cell-2',
      cell_type: 'code',
      source: '',
    },
    {
      type: 'JupyterLabCellMove',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      direction: 'sideways',
    },
    {
      type: 'JupyterLabCellMove',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      direction: 'up',
      count: 0,
    },
    {
      type: 'JupyterLabCellDuplicate',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      new_cell_id: '',
    },
    {
      type: 'JupyterLabCellDelete',
      path: 'lesson.ipynb',
      cell_id: '',
    },
    {
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
      timeout_ms: 0,
    },
    {
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: '',
      to_cell_id: 'cell-1',
    },
    { type: 'JupyterLabOpenNotebook', path: '' },
    { type: 'JupyterLabOpenNotebook', path: 'lesson.ipynb', extra: true },
    { type: 'OpenFile', path: 'main.rs' },
    { type: 'Present' },
  ]

  for (const payload of invalid) {
    assert.equal(
      parseJupyterLabAction(payload),
      undefined,
      JSON.stringify(payload),
    )
  }
})

test('a malformed payload is acknowledged as invalid_action', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(envelope(selector('')), context())

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'invalid_action')
  assert.deepEqual(facade.calls, [])
})

test('opens a Notebook and acknowledges completion', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({ type: 'JupyterLabOpenNotebook', path: 'lesson.ipynb' }),
    context(),
  )

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(facade.calls, ['open:lesson.ipynb'])
})

test('selects a Cell with the requested scroll behaviour', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({ ...selector(), scroll: false }),
    context(),
  )

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(facade.calls, ['select:lesson.ipynb:cell-1:false:auto'])
})

test('passes the requested scroll alignment to the facade', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  assert.deepEqual(
    parseJupyterLabAction({ ...selector(), align: 'top-center' }),
    {
      type: 'JupyterLabCellSelect',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      scroll: true,
      align: 'top-center',
    },
  )

  const ack = await handler(
    envelope({ ...selector(), align: 'top-center' }),
    context(),
  )

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(facade.calls, ['select:lesson.ipynb:cell-1:true:top-center'])
})

test('JupyterLabCellInsertText inserts text directly and saves', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'print(1)\n',
      speed_ms: 0,
    }),
    context(),
  )

  assert.deepEqual(ack, { status: 'done' })
  assert.deepEqual(facade.calls, [
    'insert:lesson.ipynb:cell-1:{"eof":true}:print(1)\n:0:-',
    'save:lesson.ipynb',
  ])
})

test('aborting after a cell edit but before saving skips the save', async () => {
  const facade = new FakeJupyterLabFacade()
  const controller = new AbortController()
  let releaseInsert: (() => void) | undefined
  facade.insertTextGate = new Promise<void>((resolve) => {
    releaseInsert = resolve
  })
  const handler = createJupyterLabActionHandler(facade)

  const pending = handler(
    envelope({
      type: 'JupyterLabCellInsertText',
      path: 'lesson.ipynb',
      cell_id: 'cell-1',
      target: { eof: true },
      text: 'print(1)\n',
      speed_ms: 0,
    }),
    context(controller.signal),
  )
  await waitFor(() => facade.calls.length === 1)
  controller.abort()
  releaseInsert?.()
  const ack = await pending

  assert.deepEqual(ack, { status: 'cancelled' })
  assert.deepEqual(facade.calls, [
    'insert:lesson.ipynb:cell-1:{"eof":true}:print(1)\n:0:-',
  ])
})

test('JupyterLabCellRun waits for idle, saves, and returns execution summaries only', async () => {
  const facade = new FakeJupyterLabFacade()
  facade.execution = {
    execution_count: 3,
    error: {
      ename: 'ValueError',
      evalue: 'bad input',
      traceback: ['ValueError: bad input'],
    },
    outputs: [
      { output_type: 'stream', mime_types: [], size: 6 },
      {
        output_type: 'display_data',
        mime_types: ['image/png', 'text/plain'],
        size: 4096,
      },
    ],
    text: 'ValueError: bad input\n',
  }
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-0',
      to_cell_id: 'cell-1',
      timeout_ms: 1000,
    }),
    context(),
  )

  assert.deepEqual(ack, {
    status: 'done',
    execution_count: 3,
    error: {
      ename: 'ValueError',
      evalue: 'bad input',
      traceback: ['ValueError: bad input'],
    },
    outputs: [
      { output_type: 'stream', mime_types: [], size: 6 },
      {
        output_type: 'display_data',
        mime_types: ['image/png', 'text/plain'],
        size: 4096,
      },
    ],
    output: 'ValueError: bad input\n',
    output_truncated: false,
  })
  assert.deepEqual(facade.calls, [
    'run:lesson.ipynb:cell-0:cell-1:1000',
    'save:lesson.ipynb',
  ])
})

test('JupyterLabCellRun bounds text and never forwards rich output bytes', async () => {
  const facade = new FakeJupyterLabFacade()
  facade.execution = {
    outputs: [
      { output_type: 'display_data', mime_types: ['image/png'], size: 900 },
    ],
    text: `${'x'.repeat(MAX_CELL_OUTPUT_CHARS)}TAIL`,
  }
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    }),
    context(),
  )

  assert.equal(ack.output_truncated, true)
  assert.ok((ack.output ?? '').startsWith('[Cynapse: output truncated]\n'))
  assert.ok((ack.output ?? '').endsWith('TAIL'))
  assert.ok((ack.output ?? '').length <= MAX_CELL_OUTPUT_CHARS + 32)
  assert.deepEqual(ack.outputs, [
    { output_type: 'display_data', mime_types: ['image/png'], size: 900 },
  ])
  assert.equal(boundedOutput('plain'), 'plain')
})

test('JupyterLabCellRun bounds errors and output summaries', async () => {
  const facade = new FakeJupyterLabFacade()
  const huge = 'x'.repeat(100_000)
  facade.execution = {
    error: {
      ename: 'ValueError',
      evalue: huge,
      traceback: Array.from({ length: 40 }, () => huge),
    },
    outputs: Array.from({ length: MAX_CELL_OUTPUTS + 5 }, (_, index) => ({
      output_type: 'display_data' as const,
      mime_types: Array.from({ length: 20 }, () => `image/${'x'.repeat(200)}`),
      size: index,
    })),
    text: 'ok',
  }
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    }),
    context(),
  )

  assert.equal(ack.outputs?.length, MAX_CELL_OUTPUTS)
  assert.equal(ack.outputs_truncated, true)
  assert.equal(ack.error?.evalue.length, MAX_CELL_ERROR_CHARS)
  assert.equal(ack.error?.traceback.length, MAX_CELL_TRACEBACK_LINES)
  assert.ok(
    ack.error?.traceback.every((line) => line.length <= MAX_CELL_ERROR_CHARS),
  )
  assert.ok(JSON.stringify(ack).length < 1_000_000)
})

test('JupyterLabCellRun keeps a truncated marker when the facade already bounded the tail', async () => {
  const facade = new FakeJupyterLabFacade()
  facade.execution = {
    outputs: [],
    text: 'visible tail',
    text_truncated: true,
  }
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    }),
    context(),
  )

  assert.equal(ack.output_truncated, true)
  assert.ok((ack.output ?? '').startsWith('[Cynapse: output truncated]\n'))
  assert.ok((ack.output ?? '').endsWith('visible tail'))
})

test('typed facade failures keep their error codes', async () => {
  const cases: Array<{
    action: Record<string, unknown>
    method: keyof JupyterLabFacade
    code: JupyterLabFacadeError['code']
    expected: AckPayload
  }> = [
    {
      action: { type: 'JupyterLabOpenNotebook', path: 'missing.ipynb' },
      method: 'openNotebook',
      code: 'notebook_missing',
      expected: {
        status: 'error',
        code: 'notebook_missing',
        message: 'Notebook missing',
      },
    },
    {
      action: {
        type: 'JupyterLabCellSelect',
        path: 'lesson.ipynb',
        cell_id: 'cell-1',
      },
      method: 'selectCell',
      code: 'notebook_not_open',
      expected: {
        status: 'error',
        code: 'notebook_not_open',
        message: 'Notebook not open',
      },
    },
    {
      action: {
        type: 'JupyterLabCellSelect',
        path: 'lesson.ipynb',
        cell_id: 'cell-1',
      },
      method: 'selectCell',
      code: 'cell_id_missing',
      expected: {
        status: 'error',
        code: 'cell_id_missing',
        message: 'Cell missing',
      },
    },
    {
      action: {
        type: 'JupyterLabCellSelect',
        path: 'lesson.ipynb',
        cell_id: 'cell-1',
      },
      method: 'selectCell',
      code: 'cell_id_duplicate',
      expected: {
        status: 'error',
        code: 'cell_id_duplicate',
        message: 'Cell duplicated',
      },
    },
    {
      action: {
        type: 'JupyterLabCellInsertText',
        path: 'lesson.ipynb',
        cell_id: 'cell-1',
        target: { eof: true },
        text: 'x',
        speed_ms: 0,
      },
      method: 'insertText',
      code: 'cell_id_missing',
      expected: {
        status: 'error',
        code: 'cell_id_missing',
        message: 'Cell missing',
      },
    },
    {
      action: {
        type: 'JupyterLabCellRun',
        path: 'lesson.ipynb',
        from_cell_id: 'cell-1',
        to_cell_id: 'cell-1',
      },
      method: 'runCell',
      code: 'kernel_dead',
      expected: {
        status: 'error',
        code: 'kernel_dead',
        message: 'Kernel dead',
      },
    },
    {
      action: {
        type: 'JupyterLabCellRun',
        path: 'lesson.ipynb',
        from_cell_id: 'cell-1',
        to_cell_id: 'cell-1',
      },
      method: 'runCell',
      code: 'execution_timeout',
      expected: {
        status: 'error',
        code: 'execution_timeout',
        message: 'Execution timed out',
        timed_out: true,
      },
    },
    {
      action: {
        type: 'JupyterLabCellInsertText',
        path: 'lesson.ipynb',
        cell_id: 'cell-1',
        target: { eof: true },
        text: 'x',
        speed_ms: 0,
      },
      method: 'saveNotebook',
      code: 'save_conflict',
      expected: {
        status: 'error',
        code: 'save_conflict',
        message: 'Save conflict',
      },
    },
  ]

  for (const { action, method, code, expected } of cases) {
    const facade = new FakeJupyterLabFacade()
    facade.failures.set(
      method,
      new JupyterLabFacadeError(code, expected.message!),
    )
    const handler = createJupyterLabActionHandler(facade)

    const ack = await handler(envelope(action), context())

    assert.deepEqual(ack, expected, `${method}:${code}`)
  }
})

test('a save conflict after JupyterLabCellRun is reported and does not hide the execution', async () => {
  const facade = new FakeJupyterLabFacade()
  facade.execution = { outputs: [], text: 'ok\n' }
  facade.failures.set(
    'saveNotebook',
    new JupyterLabFacadeError('save_conflict', 'Notebook changed on disk'),
  )
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    }),
    context(),
  )

  assert.deepEqual(ack, {
    status: 'error',
    code: 'save_conflict',
    message: 'Notebook changed on disk',
  })
  assert.deepEqual(facade.calls, [
    'run:lesson.ipynb:cell-1:cell-1:300000',
    'save:lesson.ipynb',
  ])
})

test('cancelling an active JupyterLabCellRun returns cancelled and skips the save', async () => {
  const facade = new FakeJupyterLabFacade()
  const controller = new AbortController()
  const handler = createJupyterLabActionHandler(facade)
  facade.runGate = new Promise<void>((resolve) => {
    controller.signal.addEventListener(
      'abort',
      () => {
        resolve()
      },
      { once: true },
    )
  })

  const pending = handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    }),
    context(controller.signal),
  )
  await waitFor(() => facade.calls.length === 1)
  controller.abort()
  const ack = await pending

  assert.deepEqual(ack, { status: 'cancelled' })
  assert.deepEqual(facade.calls, ['run:lesson.ipynb:cell-1:cell-1:300000'])
})

test('a JupyterLab Kernel restart fallback is visible on the cancelled ACK', async () => {
  const facade = new FakeJupyterLabFacade()
  facade.failures.set(
    'runCell',
    new JupyterLabFacadeError(
      'kernel_restarted',
      'The Cell did not stop; the Kernel was restarted and variables were cleared',
    ),
  )
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    }),
    context(),
  )

  assert.deepEqual(ack, {
    status: 'cancelled',
    code: 'kernel_restarted',
    message:
      'The Cell did not stop; the Kernel was restarted and variables were cleared',
  })
})

test('a failed JupyterLab Kernel restart is an error even after abort', async () => {
  const facade = new FakeJupyterLabFacade()
  const controller = new AbortController()
  facade.runCell = async () => {
    controller.abort()
    throw new JupyterLabFacadeError(
      'kernel_restart_failed',
      'The Kernel could not be restarted',
    )
  }
  const handler = createJupyterLabActionHandler(facade)

  const ack = await handler(
    envelope({
      type: 'JupyterLabCellRun',
      path: 'lesson.ipynb',
      from_cell_id: 'cell-1',
      to_cell_id: 'cell-1',
    }),
    context(controller.signal),
  )

  assert.deepEqual(ack, {
    status: 'error',
    code: 'kernel_restart_failed',
    message: 'The Kernel could not be restarted',
  })
})

test('restarts and interrupts the Notebook kernel', async () => {
  const facade = new FakeJupyterLabFacade()
  const handler = createJupyterLabActionHandler(facade)

  assert.deepEqual(
    await handler(
      envelope({ type: 'JupyterLabRestartKernel', path: 'lesson.ipynb' }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({ type: 'JupyterLabInterruptKernel', path: 'lesson.ipynb' }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(facade.calls, [
    'restart:lesson.ipynb',
    'interrupt:lesson.ipynb',
  ])
})

test('routes TerminalEnsure and TerminalCommand through the terminal facade', async () => {
  const facade = new FakeJupyterLabFacade()
  const terminalCalls: string[] = []
  const terminal: TerminalFacade = {
    async ensure(sessionKey, cwd) {
      terminalCalls.push(`ensure:${sessionKey}:${cwd}`)
    },
    async completionMarkerExists() {
      return false
    },
    async disposeUnderPaths() {},
    async execute(action) {
      terminalCalls.push(`execute:${action.session_key}:${action.command}`)
      return {
        exitCode: 0,
        output: 'ok\n',
        cancelled: false,
        timedOut: false,
      }
    },
  }
  const handler = createJupyterLabActionHandler(facade, terminal)

  assert.deepEqual(
    await handler(
      envelope({
        type: 'TerminalEnsure',
        session_key: 'unit::main',
        cwd: 'unit',
      }),
      context(),
    ),
    { status: 'done' },
  )
  assert.deepEqual(
    await handler(
      envelope({
        type: 'TerminalCommand',
        session_key: 'unit::main',
        cwd: 'unit',
        command: 'python --version',
      }),
      context(),
    ),
    {
      status: 'done',
      exit_code: 0,
      output: 'ok\n',
      output_truncated: false,
    },
  )
  assert.deepEqual(terminalCalls, [
    'ensure:unit::main:unit',
    'execute:unit::main:python --version',
  ])
  assert.deepEqual(facade.calls, [])
})

test('a terminal action without a terminal facade is explicitly unhandled', async () => {
  const handler = createJupyterLabActionHandler(new FakeJupyterLabFacade())

  const ack = await handler(
    envelope({
      type: 'TerminalEnsure',
      session_key: 'unit::main',
      cwd: 'unit',
    }),
    context(),
  )

  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'unhandled_action')
})

test('applies a parsed action directly for callers that already validated it', async () => {
  const facade = new FakeJupyterLabFacade()
  const action: JupyterLabAction = {
    type: 'JupyterLabOpenNotebook',
    path: 'lesson.ipynb',
  }

  const ack = await applyJupyterLabAction(
    action,
    facade,
    new AbortController().signal,
  )

  assert.deepEqual(ack, { status: 'done' })
})

async function waitFor(
  predicate: () => boolean,
  timeoutMs = 1000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() >= deadline) {
      throw new Error('timed out waiting for the fake facade')
    }
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
}
