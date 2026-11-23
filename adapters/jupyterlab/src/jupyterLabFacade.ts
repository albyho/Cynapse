import type { JupyterFrontEnd } from '@jupyterlab/application'
import { Notification } from '@jupyterlab/apputils'
import { CodeCell, type Cell } from '@jupyterlab/cells'
import type { IDocumentManager } from '@jupyterlab/docmanager'
import type { IFileBrowserFactory } from '@jupyterlab/filebrowser'
import {
  NotebookActions,
  type INotebookTracker,
  type Notebook,
  type NotebookPanel,
} from '@jupyterlab/notebook'
import { ServerConnection, type ServiceManager } from '@jupyterlab/services'

import { typewriterTokens } from '@cynapse/adapter-kit'

import {
  JupyterLabFacadeError,
  MAX_CELL_ERROR_CHARS,
  MAX_CELL_MIME_TYPES,
  MAX_CELL_OUTPUT_CHARS,
  MAX_CELL_OUTPUTS,
  MAX_CELL_TRACEBACK_LINES,
  type CellExecution,
  type JupyterLabFacade,
} from './actions'
import type { CheckNotebookFacade } from './check'
import type {
  CellOutputSummary,
  CellScrollAlign,
  KernelErrorPayload,
} from './protocol'
import {
  type CellDeleteTarget,
  type CellEditPlan,
  type CellEditRange,
  type CellInsertTarget,
  CellTextEditError,
  cellLineEnding,
  normalizeLineEndings,
  planCellDeleteText,
  planCellInsertText,
  planCellReplaceText,
  withLineEnding,
} from './cellTextEdit'

/** The real JupyterLab APIs behind the Adapter's facades. */
export class JupyterLabNotebookFacade
  implements JupyterLabFacade, CheckNotebookFacade
{
  /** Session ids captured before a Notebook is closed; reset shuts them down. */
  private readonly closedSessions = new Map<string, string>()

  constructor(
    private readonly app: JupyterFrontEnd,
    private readonly tracker: INotebookTracker,
    private readonly documents: IDocumentManager,
    private readonly services: ServiceManager.IManager,
    private readonly fileBrowserFactory: IFileBrowserFactory,
  ) {}

  openNotebookPaths(): readonly string[] {
    return this.tracker.filter(() => true).map((panel) => panel.context.path)
  }

  async dirtyNotebooks(): Promise<string[]> {
    return this.tracker
      .filter((panel) => panel.context.model.dirty)
      .map((panel) => panel.context.path)
  }

  async openNotebook(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const existing = this.findNotebook(path)
    if (existing) {
      this.app.shell.activateById(existing.id)
      await this.ensureNotebookReady(existing, signal)
      await this.ensureKernel(existing, signal)
      await this.revealInFileBrowser(existing.context.path)
      return
    }

    const normalized = normalizePath(path)
    try {
      const model = await this.documents.services.contents.get(normalized, {
        content: false,
      })
      if (model.type !== 'notebook') {
        throw new Error(`${normalized} is not a Notebook`)
      }
    } catch (error) {
      throw new JupyterLabFacadeError(
        'notebook_missing',
        `JupyterLab could not open ${path}: ${errorMessage(error)}`,
      )
    }

    const widget = this.documents.openOrReveal(normalized, 'Notebook')
    if (!widget) {
      throw new JupyterLabFacadeError(
        'notebook_missing',
        `JupyterLab could not open ${path}`,
      )
    }
    this.app.shell.activateById(widget.id)
    const panel = widget as NotebookPanel
    try {
      await withDeadline(panel.context.ready, 10_000)
    } catch (error) {
      void this.documents.closeFile(normalized)
      throw new JupyterLabFacadeError(
        'notebook_missing',
        `JupyterLab could not open ${path}: ${errorMessage(error)}`,
      )
    }
    throwIfAborted(signal)
    await this.ensureKernel(panel, signal)
    console.info(
      '[cynapse] notebook.opened',
      normalized,
      availableCellIds(panel),
    )
    await this.revealInFileBrowser(normalized)
  }

  async selectCell(
    path: string,
    cellId: string,
    scroll: boolean,
    align: CellScrollAlign,
    signal: AbortSignal,
  ): Promise<void> {
    throwIfAborted(signal)
    const panel = await this.ensureNotebook(path, signal)
    await this.ensureNotebookReady(panel, signal)
    const { cell, index } = findCell(panel, cellId)
    panel.content.activeCellIndex = index
    panel.content.select(cell)
    if (scroll) {
      await panel.content.scrollToCell(cell, align)
    }
  }

  async openPreview(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const normalized = normalizePath(path)
    if (!/\.(md|markdown)$/i.test(normalized)) {
      throw new JupyterLabFacadeError(
        'preview_not_markdown',
        `${path} is not a Markdown file`,
      )
    }
    try {
      const model = await this.documents.services.contents.get(normalized, {
        content: false,
      })
      if (model.type !== 'file') {
        throw new Error(`${normalized} is not a file`)
      }
    } catch (error) {
      throw new JupyterLabFacadeError(
        'preview_missing',
        `JupyterLab could not open ${path}: ${errorMessage(error)}`,
      )
    }
    throwIfAborted(signal)
    try {
      // shortcut: preview is a normal tab; side-by-side shell split is the next
      // spike if the tab layout is not sufficient for teaching.
      const factory =
        this.documents.registry.defaultRenderedWidgetFactory(normalized)
      const widget = this.documents.openOrReveal(normalized, factory.name)
      if (!widget) {
        throw new Error(`no widget factory opened ${normalized}`)
      }
      this.app.shell.activateById(widget.id)
    } catch (error) {
      throw new JupyterLabFacadeError(
        'preview_missing',
        `JupyterLab could not render ${path}: ${errorMessage(error)}`,
      )
    }
    await this.revealInFileBrowser(normalized)
  }

  async closeDocument(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const panel = this.findNotebook(path)
    if (panel) {
      if (panel.context.model.dirty) {
        await this.saveNotebook(path, signal)
      }
      await this.closeNotebook(path, signal)
      await this.shutdownKernel(path, signal)
      return
    }

    const normalized = normalizePath(path)
    if (this.documents.findWidget(normalized)) {
      await this.documents.closeFile(normalized)
    }
  }

  async insertText(
    path: string,
    cellId: string,
    target: CellInsertTarget,
    text: string,
    speedMs: number,
    targetHoldMs: number | undefined,
    signal: AbortSignal,
  ): Promise<void> {
    await this.editCellText(
      path,
      cellId,
      signal,
      (source) => planCellInsertText(source, target, text),
      speedMs,
      targetHoldMs,
    )
  }

  async deleteText(
    path: string,
    cellId: string,
    target: CellDeleteTarget,
    direction: 'backspace' | 'delete',
    speedMs: number,
    targetHoldMs: number | undefined,
    signal: AbortSignal,
  ): Promise<void> {
    await this.editCellText(
      path,
      cellId,
      signal,
      (source) => planCellDeleteText(source, target, direction),
      speedMs,
      targetHoldMs,
    )
  }

  async replaceText(
    path: string,
    cellId: string,
    target: CellEditRange,
    text: string,
    speedMs: number,
    targetHoldMs: number | undefined,
    signal: AbortSignal,
  ): Promise<void> {
    await this.editCellText(
      path,
      cellId,
      signal,
      (source) => planCellReplaceText(source, target, text),
      speedMs,
      targetHoldMs,
    )
  }

  private async editCellText(
    path: string,
    cellId: string,
    signal: AbortSignal,
    plan: (source: string) => CellEditPlan,
    speedMs: number,
    targetHoldMs: number | undefined,
  ): Promise<void> {
    throwIfAborted(signal)
    const panel = await this.ensureNotebook(path, signal)
    await this.ensureNotebookReady(panel, signal)
    const { cell, index } = findCell(panel, cellId)
    panel.content.activeCellIndex = index
    panel.content.select(cell)
    await panel.content.scrollToCell(cell)

    const sharedModel = cell.model.sharedModel
    const editor = cell.editor
    const original = sharedModel.getSource()
    const source = normalizeLineEndings(original)
    const lineEnding = cellLineEnding(original)
    const toCell = (text: string) => withLineEnding(text, lineEnding)
    let edit: CellEditPlan
    try {
      edit = plan(source)
    } catch (error) {
      throw new JupyterLabFacadeError(
        'cell_text_target_invalid',
        error instanceof CellTextEditError
          ? error.message
          : errorMessage(error),
      )
    }
    if (targetHoldMs !== undefined) {
      await delay(targetHoldMs, signal)
    }
    if (speedMs === 0 || edit.next === source) {
      sharedModel.setSource(toCell(edit.next))
      editor?.focus()
      return
    }

    const prefix = source.slice(0, edit.start)
    const suffix = source.slice(edit.end)
    if (edit.end > edit.start) {
      const removed = Array.from(source.slice(edit.start, edit.end))
      for (let index = removed.length - 1; index >= 0; index -= 1) {
        throwIfAborted(signal)
        sharedModel.setSource(
          toCell(`${prefix}${removed.slice(0, index).join('')}${suffix}`),
        )
        await delay(speedMs, signal)
      }
    }
    let typed = ''
    const startsAtLineStart =
      edit.start === 0 || source[edit.start - 1] === '\n'
    for (const token of typewriterTokens(edit.text, startsAtLineStart)) {
      throwIfAborted(signal)
      typed += token
      sharedModel.setSource(toCell(`${prefix}${typed}${suffix}`))
      const position = editor?.getPositionAt(toCell(`${prefix}${typed}`).length)
      if (editor && position) {
        editor.setCursorPosition(position)
        editor.revealPosition(position)
      }
      await delay(speedMs, signal)
    }
    sharedModel.setSource(toCell(edit.next))
    editor?.focus()
  }

  async insertCell(
    path: string,
    cellId: string,
    afterCellId: string | null,
    beforeCellId: string | null,
    cellType: 'code' | 'markdown',
    source: string,
    signal: AbortSignal,
  ): Promise<void> {
    throwIfAborted(signal)
    const panel = await this.ensureNotebook(path, signal)
    await this.ensureNotebookReady(panel, signal)
    const existing = findCellOptional(panel, cellId)
    if (existing) {
      if (existing.cell.model.type !== cellType) {
        throw new JupyterLabFacadeError(
          'cell_type_mismatch',
          `Cell ${cellId} is ${existing.cell.model.type}, not ${cellType}`,
        )
      }
      existing.cell.model.sharedModel.setSource(source)
      panel.content.activeCellIndex = existing.index
      panel.content.select(existing.cell)
      return
    }

    if (afterCellId && beforeCellId) {
      throw new JupyterLabFacadeError(
        'cell_id_duplicate',
        'JupyterLabCellInsert cannot use both before_cell_id and after_cell_id',
      )
    }
    const index = beforeCellId
      ? findCell(panel, beforeCellId).index
      : afterCellId
        ? findCell(panel, afterCellId).index + 1
        : panel.context.model.sharedModel.cells.length
    panel.context.model.sharedModel.insertCell(index, {
      cell_type: cellType,
      id: cellId,
      source,
      metadata: { cynapse_generated: true },
    })
    const inserted = await waitForCell(panel, cellId, signal)
    panel.content.activeCellIndex = inserted.index
    panel.content.select(inserted.cell)
    await panel.content.scrollToCell(inserted.cell)
  }

  async moveCell(
    path: string,
    cellId: string,
    direction: 'up' | 'down',
    count: number,
    signal: AbortSignal,
  ): Promise<void> {
    throwIfAborted(signal)
    const panel = await this.ensureNotebook(path, signal)
    await this.ensureNotebookReady(panel, signal)
    const { index } = findCell(panel, cellId)
    const last = panel.context.model.sharedModel.cells.length - 1
    const target =
      direction === 'up'
        ? Math.max(0, index - count)
        : Math.min(last, index + count)
    if (target === index) {
      return
    }
    panel.content.moveCell(index, target)
    const moved = findCell(panel, cellId)
    panel.content.activeCellIndex = moved.index
    panel.content.select(moved.cell)
  }

  async duplicateCell(
    path: string,
    cellId: string,
    newCellId: string | null,
    signal: AbortSignal,
  ): Promise<void> {
    throwIfAborted(signal)
    const panel = await this.ensureNotebook(path, signal)
    await this.ensureNotebookReady(panel, signal)
    const source = findCell(panel, cellId)
    const cellType = source.cell.model.type
    if (cellType !== 'code' && cellType !== 'markdown') {
      throw new JupyterLabFacadeError(
        'cell_type_mismatch',
        `Cell ${cellId} has unsupported type ${cellType}`,
      )
    }
    const copyId = newCellId ?? `${cellId}_copy`
    if (copyId === cellId) {
      throw new JupyterLabFacadeError(
        'cell_id_duplicate',
        'Duplicate Cell id must differ from the source Cell id',
      )
    }
    const copySource = source.cell.model.sharedModel.getSource()
    const existing = findCellOptional(panel, copyId)
    if (existing) {
      if (existing.cell.model.type !== cellType) {
        throw new JupyterLabFacadeError(
          'cell_type_mismatch',
          `Cell ${copyId} is ${existing.cell.model.type}, not ${cellType}`,
        )
      }
      existing.cell.model.sharedModel.setSource(copySource)
      panel.content.activeCellIndex = existing.index
      panel.content.select(existing.cell)
      return
    }
    await this.insertCell(
      path,
      copyId,
      cellId,
      null,
      cellType,
      copySource,
      signal,
    )
  }

  async deleteCell(
    path: string,
    cellId: string,
    signal: AbortSignal,
  ): Promise<void> {
    throwIfAborted(signal)
    const panel = await this.ensureNotebook(path, signal)
    await this.ensureNotebookReady(panel, signal)
    const existing = findCellOptional(panel, cellId)
    if (!existing) {
      return
    }
    if (existing.cell.model.getMetadata('cynapse_generated') !== true) {
      throw new JupyterLabFacadeError(
        'cell_not_generated',
        `Cell ${cellId} was not inserted by Cynapse`,
      )
    }
    panel.context.model.sharedModel.deleteCell(existing.index)
  }

  async saveNotebook(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const panel = this.requireNotebook(path)
    try {
      await panel.context.save()
    } catch (error) {
      throw new JupyterLabFacadeError(
        'save_conflict',
        `JupyterLab could not save ${path}: ${errorMessage(error)}`,
      )
    }
  }

  async runCell(
    path: string,
    fromCellId: string | null,
    toCellId: string | null,
    timeoutMs: number,
    signal: AbortSignal,
  ): Promise<CellExecution> {
    return this.withExecutionNotice(() =>
      this.runCellInternal(path, fromCellId, toCellId, timeoutMs, signal),
    )
  }

  private async runCellInternal(
    path: string,
    fromCellId: string | null,
    toCellId: string | null,
    timeoutMs: number,
    signal: AbortSignal,
  ): Promise<CellExecution> {
    throwIfAborted(signal)
    const panel = await this.ensureNotebook(path, signal)
    await this.ensureNotebookReady(panel, signal)
    const cells = this.selectCells(panel, fromCellId, toCellId)
    await this.ensureKernel(panel, signal)
    throwIfAborted(signal)

    const deadline = Date.now() + timeoutMs
    for (const cell of cells) {
      throwIfAborted(signal)
      const remaining = deadline - Date.now()
      if (remaining <= 0) {
        throw new JupyterLabFacadeError(
          'execution_timeout',
          `The Cell range did not finish within ${timeoutMs} ms`,
        )
      }
      const execution = NotebookActions.runCells(
        panel.content,
        [cell],
        panel.sessionContext,
      )
      await this.awaitExecution(panel, execution, remaining, signal)
      await this.awaitKernelIdle(panel, remaining, signal)

      if (!(cell instanceof CodeCell)) {
        continue
      }
      const summary = summarizeCodeCell(cell)
      const isTarget =
        toCellId === null
          ? cell === cells[cells.length - 1]
          : cell.model.id === toCellId
      if (!isTarget && summary.error) {
        const failure = `${summary.error.ename}: ${summary.error.evalue}`
        throw new JupyterLabFacadeError(
          'cell_execution_failed',
          `Cell ${cell.model.id} failed before ${toCellId ?? 'the last Cell'}: ${failure}`,
        )
      }
    }

    const executed = cells[cells.length - 1]
    if (!(executed instanceof CodeCell)) {
      return { outputs: [], text: '' }
    }
    await this.revealExecutionResult(panel, executed, signal)
    return summarizeCodeCell(executed)
  }

  /** Show progress only when execution is slow enough to feel stalled. */
  private async withExecutionNotice<T>(run: () => Promise<T>): Promise<T> {
    let notice: string | undefined
    const timer = setTimeout(() => {
      notice = Notification.emit('Running code…', 'in-progress', {
        autoClose: false,
      })
    }, 1000)
    try {
      return await run()
    } finally {
      clearTimeout(timer)
      if (notice) {
        Notification.dismiss(notice)
      }
    }
  }

  /** Keep the last execution result visible after the CellRun completes. */
  private async revealExecutionResult(
    panel: NotebookPanel,
    cell: CodeCell,
    signal: AbortSignal,
  ): Promise<void> {
    throwIfAborted(signal)
    this.app.shell.activateById(panel.id)
    // Let JupyterLab commit the final output DOM before measuring it.
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
    throwIfAborted(signal)
    if (cell.model.outputs.length > 0) {
      cell.outputArea.node.scrollIntoView({ block: 'end', inline: 'nearest' })
    } else {
      await panel.content.scrollToCell(cell, 'end')
    }
  }

  async restartKernel(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const panel = this.requireNotebook(path)
    await this.ensureKernel(panel, signal)
    try {
      await panel.sessionContext.restartKernel()
    } catch (error) {
      throw new JupyterLabFacadeError(
        'kernel_dead',
        `JupyterLab could not restart the Kernel for ${path}: ${errorMessage(error)}`,
      )
    }
  }

  async interruptKernel(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const panel = this.requireNotebook(path)
    const kernel = panel.sessionContext.session?.kernel
    if (!kernel) {
      throw new JupyterLabFacadeError(
        'kernel_dead',
        `The Notebook ${path} has no running Kernel`,
      )
    }
    try {
      await kernel.interrupt()
    } catch (error) {
      throw new JupyterLabFacadeError(
        'kernel_dead',
        `JupyterLab could not interrupt the Kernel for ${path}: ${errorMessage(error)}`,
      )
    }
  }

  /**
   * Reset discards unsaved Notebook changes after the Client confirmed. Clear
   * the dirty flag first so JupyterLab does not show its own save dialog.
   */
  async closeNotebook(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const panel = this.findNotebook(path)
    if (!panel) {
      return
    }

    const sessionId = panel.sessionContext.session?.id
    const key = normalizePath(path)
    if (sessionId) {
      this.closedSessions.set(key, sessionId)
    }
    panel.context.model.dirty = false
    await this.documents.closeFile(key)
  }

  async shutdownKernel(path: string, signal: AbortSignal): Promise<void> {
    throwIfAborted(signal)
    const key = normalizePath(path)
    const sessionId = this.closedSessions.get(key)
    this.closedSessions.delete(key)
    if (!sessionId) {
      return
    }

    try {
      await this.services.sessions.shutdown(sessionId)
    } catch (error) {
      if (isNotFound(error)) {
        return
      }
      throw new JupyterLabFacadeError(
        'kernel_shutdown_incomplete',
        `JupyterLab could not shut down the Kernel for ${path}: ${errorMessage(error)}`,
      )
    }
  }

  private async revealInFileBrowser(path: string): Promise<void> {
    const browser = this.fileBrowserFactory.tracker.currentWidget
    if (!browser) {
      return
    }
    const normalized = normalizePath(path)
    const separator = normalized.lastIndexOf('/')
    const directory = separator < 0 ? '/' : `/${normalized.slice(0, separator)}`
    const name = normalized.slice(separator + 1)
    try {
      await browser.model.cd(directory)
      await browser.selectItemByName(name)
      this.app.shell.activateById(browser.id)
    } catch (error) {
      console.info(
        '[cynapse] filebrowser.reveal.failed',
        normalized,
        errorMessage(error),
      )
    }
  }

  private findNotebook(path: string): NotebookPanel | undefined {
    const target = normalizePath(path)
    return this.tracker.find((panel) => panel.context.path === target)
  }

  private requireNotebook(path: string): NotebookPanel {
    const panel = this.findNotebook(path)
    if (!panel) {
      throw new JupyterLabFacadeError(
        'notebook_not_open',
        `The Notebook ${path} is not open in JupyterLab`,
      )
    }
    return panel
  }

  private async ensureNotebook(
    path: string,
    signal: AbortSignal,
  ): Promise<NotebookPanel> {
    const existing = this.findNotebook(path)
    if (existing) {
      return existing
    }
    // Cell and run actions are self-contained: open the Notebook instead of
    // relying on a previous Step or a restored JupyterLab tab. Kernel actions
    // must not start a Notebook just to restart or interrupt it.
    await this.openNotebook(path, signal)
    const opened = this.requireNotebook(path)
    this.app.shell.activateById(opened.id)
    return opened
  }

  private selectCells(
    panel: NotebookPanel,
    fromCellId: string | null,
    toCellId: string | null,
  ): Cell[] {
    const cells = panel.content.widgets
    if (cells.length === 0) {
      return []
    }
    const start = fromCellId === null ? 0 : findCell(panel, fromCellId).index
    const end =
      toCellId === null ? cells.length - 1 : findCell(panel, toCellId).index
    if (start > end) {
      throw new JupyterLabFacadeError(
        'cell_range_invalid',
        `Cell ${fromCellId} is after ${toCellId} in ${panel.context.path}`,
      )
    }
    return cells
      .slice(start, end + 1)
      .filter((cell): cell is CodeCell => cell instanceof CodeCell)
  }

  private async ensureNotebookReady(
    panel: NotebookPanel,
    signal: AbortSignal,
  ): Promise<void> {
    throwIfAborted(signal)
    if (panel.context.isReady) {
      return
    }
    try {
      await withDeadline(panel.context.ready, 10_000)
    } catch (error) {
      throw new JupyterLabFacadeError(
        'notebook_not_open',
        `JupyterLab did not finish loading ${panel.context.path}: ${errorMessage(error)}`,
      )
    }
    throwIfAborted(signal)
  }

  private async ensureKernel(
    panel: NotebookPanel,
    signal: AbortSignal,
  ): Promise<void> {
    const context = panel.sessionContext
    if (!context.isReady) {
      const needsSelection = await context.initialize().catch(() => false)
      if (needsSelection) {
        await this.startDefaultKernel(context)
      }
    }
    throwIfAborted(signal)
    if (!context.session?.kernel) {
      await this.startDefaultKernel(context)
    }
    if (!context.session?.kernel) {
      throw new JupyterLabFacadeError(
        'kernel_dead',
        `The Notebook ${panel.context.path} has no running Kernel`,
      )
    }
  }

  private async startDefaultKernel(
    context: NotebookPanel['sessionContext'],
  ): Promise<void> {
    await this.services.kernelspecs.ready.catch(() => undefined)
    const specs = this.services.kernelspecs.specs
    const name = specs?.default ?? Object.keys(specs?.kernelspecs ?? {})[0]
    if (!name) {
      throw new JupyterLabFacadeError(
        'kernel_dead',
        'JupyterLab has no Kernel available for this Notebook',
      )
    }
    try {
      await context.changeKernel({ name })
    } catch (error) {
      throw new JupyterLabFacadeError(
        'kernel_dead',
        `JupyterLab could not start a Kernel: ${errorMessage(error)}`,
      )
    }
  }

  private awaitExecution(
    panel: NotebookPanel,
    execution: Promise<boolean>,
    timeoutMs: number,
    signal: AbortSignal,
  ): Promise<boolean> {
    return new Promise((resolve, reject) => {
      let finished = false
      let stopping: 'abort' | 'timeout' | undefined
      let hardStopped = false
      let interruptTimer: ReturnType<typeof setInterval> | undefined
      let hardStopTimer: ReturnType<typeof setTimeout> | undefined
      let timeoutTimer: ReturnType<typeof setTimeout> | undefined

      const interrupt = (): void => {
        void panel.sessionContext.session?.kernel
          ?.interrupt()
          .catch(() => undefined)
      }
      const cleanup = (): void => {
        if (timeoutTimer) {
          clearTimeout(timeoutTimer)
        }
        if (interruptTimer) {
          clearInterval(interruptTimer)
        }
        if (hardStopTimer) {
          clearTimeout(hardStopTimer)
        }
        signal.removeEventListener('abort', onAbort)
      }
      const finish = (error?: Error, value?: boolean): void => {
        if (finished) {
          return
        }
        finished = true
        cleanup()
        if (error) {
          reject(error)
        } else {
          resolve(value === true)
        }
      }
      const interruptedError = (): Error =>
        stopping === 'timeout'
          ? new JupyterLabFacadeError(
              'execution_timeout',
              `The Cell did not finish within ${timeoutMs} ms`,
            )
          : new Error('aborted')
      const stopError = (kernelRestarted: boolean): JupyterLabFacadeError => {
        if (stopping === 'timeout') {
          return new JupyterLabFacadeError(
            kernelRestarted ? 'execution_timeout' : 'kernel_restart_failed',
            kernelRestarted
              ? `The Cell did not finish within ${timeoutMs} ms; the Kernel was restarted and variables were cleared`
              : `The Cell did not finish within ${timeoutMs} ms and the Kernel could not be restarted`,
          )
        }
        return new JupyterLabFacadeError(
          kernelRestarted ? 'kernel_restarted' : 'kernel_restart_failed',
          kernelRestarted
            ? 'The Cell did not stop within 5 seconds; the Kernel was restarted and variables were cleared'
            : 'The Cell did not stop within 5 seconds and the Kernel could not be restarted',
        )
      }
      const beginStop = (kind: 'abort' | 'timeout'): void => {
        if (stopping) {
          return
        }
        stopping = kind
        if (timeoutTimer) {
          clearTimeout(timeoutTimer)
        }
        interrupt()
        // A cancel can arrive before JupyterLab has sent execute_request. Keep
        // interrupting until the queued execution starts and stops.
        interruptTimer = setInterval(interrupt, 250)
        // Windows Kernel signals can be ignored. Restart before the ACK so a
        // stopped Action cannot leave a busy Kernel behind.
        hardStopTimer = setTimeout(() => {
          hardStopped = true
          if (interruptTimer) {
            clearInterval(interruptTimer)
          }
          void withDeadline(panel.sessionContext.restartKernel(), 15_000).then(
            () => finish(stopError(true)),
            () => finish(stopError(false)),
          )
        }, 5_000)
      }
      const onAbort = (): void => beginStop('abort')

      if (signal.aborted) {
        onAbort()
      } else {
        signal.addEventListener('abort', onAbort, { once: true })
        timeoutTimer = setTimeout(() => beginStop('timeout'), timeoutMs)
      }

      execution.then(
        (value) => {
          if (hardStopped) {
            return
          }
          if (stopping) {
            finish(interruptedError())
          } else {
            finish(undefined, value)
          }
        },
        (error) => {
          if (hardStopped) {
            return
          }
          if (stopping) {
            finish(interruptedError())
          } else {
            finish(error instanceof Error ? error : new Error(String(error)))
          }
        },
      )
    })
  }

  /** IOPub outputs precede idle, so idle means the Cell summary is complete. */
  private awaitKernelIdle(
    panel: NotebookPanel,
    timeoutMs: number,
    signal: AbortSignal,
  ): Promise<void> {
    const kernel = panel.sessionContext.session?.kernel
    if (!kernel || kernel.status === 'idle') {
      return Promise.resolve()
    }

    return new Promise((resolve) => {
      let finished = false
      const done = (): void => {
        if (finished) {
          return
        }
        finished = true
        clearTimeout(timer)
        kernel.statusChanged.disconnect(onStatus)
        signal.removeEventListener('abort', done)
        resolve()
      }
      const onStatus = (): void => {
        if (kernel.status === 'idle') {
          done()
        }
      }
      const timer = setTimeout(done, Math.min(timeoutMs, 5_000))
      kernel.statusChanged.connect(onStatus)
      signal.addEventListener('abort', done, { once: true })
    })
  }
}

function findCellOptional(
  panel: NotebookPanel,
  cellId: string,
): { cell: Cell; index: number } | undefined {
  const notebook: Notebook = panel.content
  const matches: { cell: Cell; index: number }[] = []
  notebook.widgets.forEach((cell, index) => {
    if (cell.model.id === cellId) {
      matches.push({ cell, index })
    }
  })
  if (matches.length > 1) {
    throw new JupyterLabFacadeError(
      'cell_id_duplicate',
      `Cell ${cellId} appears ${matches.length} times in ${panel.context.path}`,
    )
  }
  return matches[0]
}

function findCell(
  panel: NotebookPanel,
  cellId: string,
): { cell: Cell; index: number } {
  const found = findCellOptional(panel, cellId)
  if (!found) {
    throw new JupyterLabFacadeError(
      'cell_id_missing',
      `Cell ${cellId} is not in ${panel.context.path}; available cells: ${availableCellIds(panel)}`,
    )
  }
  return found
}

function availableCellIds(panel: NotebookPanel): string {
  const ids = panel.content.widgets.map((cell) => cell.model.id)
  return ids.length === 0 ? '(none)' : ids.slice(0, 20).join(', ')
}

async function waitForCell(
  panel: NotebookPanel,
  cellId: string,
  signal: AbortSignal,
): Promise<{ cell: Cell; index: number }> {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    throwIfAborted(signal)
    const found = findCellOptional(panel, cellId)
    if (found) {
      return found
    }
    await delay(10, signal)
  }
  throw new JupyterLabFacadeError(
    'cell_id_missing',
    `Inserted Cell ${cellId} did not appear in ${panel.context.path}`,
  )
}

function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new Error('aborted'))
      return
    }
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    const onAbort = (): void => {
      clearTimeout(timer)
      reject(new Error('aborted'))
    }
    signal.addEventListener('abort', onAbort, { once: true })
  })
}

interface RawOutput {
  output_type?: unknown
  text?: unknown
  data?: Record<string, unknown>
  ename?: unknown
  evalue?: unknown
  traceback?: unknown
}

function summarizeCodeCell(cell: CodeCell): CellExecution {
  const outputs: CellOutputSummary[] = []
  let text = ''
  let textTruncated = false
  let outputsTruncated = false
  let error: KernelErrorPayload | undefined

  const appendText = (chunk: string) => {
    if (!chunk) {
      return
    }
    if (chunk.length >= MAX_CELL_OUTPUT_CHARS) {
      text = chunk.slice(-MAX_CELL_OUTPUT_CHARS)
      textTruncated = true
      return
    }
    const combined = text + chunk
    if (combined.length > MAX_CELL_OUTPUT_CHARS) {
      text = combined.slice(-MAX_CELL_OUTPUT_CHARS)
      textTruncated = true
      return
    }
    text = combined
  }
  const appendTextValue = (value: unknown): number => {
    if (typeof value === 'string') {
      appendText(value)
      return value.length
    }
    if (!Array.isArray(value)) {
      return 0
    }
    let length = 0
    for (const item of value) {
      length += appendTextValue(item)
    }
    return length
  }
  const addOutput = (output: CellOutputSummary) => {
    if (outputs.length >= MAX_CELL_OUTPUTS) {
      outputsTruncated = true
      return
    }
    outputs.push(output)
  }

  for (const output of cell.outputArea.model.toJSON() as RawOutput[]) {
    if (output.output_type === 'stream') {
      const streamLength = appendTextValue(output.text)
      addOutput({
        output_type: 'stream',
        mime_types: [],
        size: streamLength,
      })
    } else if (output.output_type === 'error') {
      const traceback = (
        Array.isArray(output.traceback) ? output.traceback.map(String) : []
      )
        .slice(-MAX_CELL_TRACEBACK_LINES)
        .map((line) => line.slice(0, MAX_CELL_ERROR_CHARS))
      error = {
        ename: String(output.ename ?? '').slice(0, MAX_CELL_ERROR_CHARS),
        evalue: String(output.evalue ?? '').slice(0, MAX_CELL_ERROR_CHARS),
        traceback,
      }
      const errorText = traceback.join('\n')
      appendText(errorText || `${error.ename}: ${error.evalue}`)
      addOutput({
        output_type: 'error',
        mime_types: [],
        size: errorText.length,
      })
    } else if (
      output.output_type === 'execute_result' ||
      output.output_type === 'display_data'
    ) {
      const data = output.data ?? {}
      const mimeTypes = Object.keys(data).sort().slice(0, MAX_CELL_MIME_TYPES)
      appendTextValue(data['text/plain'])
      addOutput({
        output_type: output.output_type,
        mime_types: mimeTypes,
        size: dataSize(data),
      })
    }
  }

  const executionCount = cell.model.executionCount
  return {
    outputs,
    text,
    text_truncated: textTruncated,
    outputs_truncated: outputsTruncated,
    ...(executionCount === null ? {} : { execution_count: executionCount }),
    ...(error ? { error } : {}),
  }
}

function dataSize(data: Record<string, unknown>): number {
  let total = 0
  for (const value of Object.values(data)) {
    total += valueSize(value, MAX_CELL_OUTPUT_CHARS - total)
    if (total >= MAX_CELL_OUTPUT_CHARS) {
      return MAX_CELL_OUTPUT_CHARS
    }
  }
  return total
}

function valueSize(value: unknown, limit: number): number {
  if (limit <= 0) {
    return 0
  }
  if (typeof value === 'string') {
    return Math.min(value.length, limit)
  }
  if (Array.isArray(value)) {
    let total = 0
    for (const item of value) {
      total += valueSize(item, limit - total)
      if (total >= limit) {
        return limit
      }
    }
    return total
  }
  if (value !== null && typeof value === 'object') {
    let total = 0
    for (const [key, item] of Object.entries(value)) {
      total += key.length
      if (total >= limit) {
        return limit
      }
      total += valueSize(item, limit - total)
      if (total >= limit) {
        return limit
      }
    }
    return total
  }
  return 0
}

function isNotFound(error: unknown): boolean {
  return (
    error instanceof ServerConnection.ResponseError &&
    error.response.status === 404
  )
}

function withDeadline<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`Timed out after ${timeoutMs} ms`)),
      timeoutMs,
    )
    promise.then(
      (value) => {
        clearTimeout(timer)
        resolve(value)
      },
      (error) => {
        clearTimeout(timer)
        reject(error)
      },
    )
  })
}

function normalizePath(path: string): string {
  return path
    .replace(/\\/g, '/')
    .replace(/^\.\/+/, '')
    .replace(/^\/+/, '')
}

function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) {
    throw new Error('aborted')
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
