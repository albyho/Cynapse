import * as path from 'node:path'
import * as vscode from 'vscode'

import {
  ApplyError,
  matchTextPrefix,
  type TextMatch,
  type EditorFacade,
  type InsertOptions,
  type Position,
  type PreparedTarget,
  type PreviewView,
} from './apply'
import { t } from './i18n'
import {
  resolveWorkspacePath,
  type FileSystemFacade,
  type TargetKind,
} from './filesystem'
import {
  isMarkdownPreviewViewType,
  pruneRecordedPreviews,
  tabsForFile,
} from './tabs'
import { workspaceRootFromConfig } from './workspace'
import { resolveRealWorkspacePath, samePath } from './workspacePaths'

export interface OpenedPreview {
  /** 预览 tab 对象本身：不同单元的同名文件（如 INTRO.md）靠它区分。 */
  tab: vscode.Tab
  filePath: string
}

/**
 * 由 OpenPreview 打开过的 Markdown 预览。
 * TabInputWebview 只有 viewType、没有 uri，也没法从标题可靠区分同名文件，
 * 因此在打开时记住 tab 对象与完整路径的对应关系。
 */
export const openedPreviews: OpenedPreview[] = []

function allTabs(): vscode.Tab[] {
  return vscode.window.tabGroups.all.flatMap((group) => [...group.tabs])
}

/** Called by the extension when tabs open, close, or move. */
export function pruneOpenedPreviews(): void {
  pruneRecordedPreviews(openedPreviews, allTabs())
}

function isPreviewTab(tab: vscode.Tab | undefined): tab is vscode.Tab {
  if (!tab) {
    return false
  }
  if (tab.input instanceof vscode.TabInputWebview) {
    return true
  }
  // 某些形态的预览是带 viewType 的自定义编辑器，也记下来。
  const viewType = (tab.input as { viewType?: unknown } | undefined)?.viewType
  return typeof viewType === 'string' && isMarkdownPreviewViewType(viewType)
}

async function locatePreviewTab(
  before: ReadonlySet<vscode.Tab>,
): Promise<vscode.Tab | undefined> {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const added = allTabs().find((tab) => !before.has(tab) && isPreviewTab(tab))
    if (added) {
      return added
    }
    await new Promise<void>((resolve) => setTimeout(resolve, 20))
  }
  // 复用已有预览 panel 时不会产生新 tab；showPreview 会把它激活。
  const active = vscode.window.tabGroups.activeTabGroup.activeTab
  return isPreviewTab(active) ? active : undefined
}

/**
 * 写入前的磁盘校验：磁盘内容必须仍是打开时记下的准备基线，或者就等于当前
 * 编辑器文本（说明磁盘上的差异正是本次编辑本身）。两者都不是，意味着有
 * 外部进程改过这个文件，此时覆盖会丢数据。
 */
function canSafelyWriteDocumentSnapshot(
  preparedText: string,
  diskText: string | undefined,
  currentText: string,
): boolean {
  return diskText === preparedText || diskText === currentText
}

class VSCodeFacade implements EditorFacade, FileSystemFacade {
  private readonly editors = new Map<string, vscode.TextEditor>()
  private revealDecoration?: vscode.TextEditorDecorationType
  private decoratedEditor?: vscode.TextEditor

  constructor(private readonly configuredWorkspaceRoot?: string) {}

  private workspaceRoot(): string {
    const root = workspaceRootFromConfig(this.configuredWorkspaceRoot)
    if (!root) {
      throw new ApplyError(
        'workspace_root_missing',
        'Open a workspace before playing a course',
      )
    }
    return root
  }

  private resolveRequestedPath(requestedPath: string): string {
    return resolveWorkspacePath(this.workspaceRoot(), requestedPath)
  }

  private async safeResolvedPath(requestedPath: string): Promise<string> {
    return resolveRealWorkspacePath(
      this.workspaceRoot(),
      this.resolveRequestedPath(requestedPath),
    )
  }

  private getEditor(target: PreparedTarget): vscode.TextEditor {
    const editor = this.editors.get(target.uri)
    if (!editor || editor.document.isClosed) {
      this.editors.delete(target.uri)
      throw new ApplyError(
        'document_closed',
        'The target document was closed during playback',
      )
    }
    return editor
  }

  // Closed documents would otherwise stay in the map for the whole session.
  private pruneClosedEditors(): void {
    for (const [uri, editor] of this.editors) {
      if (editor.document.isClosed) {
        this.editors.delete(uri)
      }
    }
  }

  /**
   * 编辑/选中前，确保目标文档所在的 tab 处于可见状态（但不抢占键盘焦点）。
   * 否则当代码文件位于后台 tab 时，revealRange 与 selection 都不会产生可见效果。
   * 仅在文档当前不可见或不是活动编辑器时才调用 showTextDocument，避免逐字
   * 打字时反复抖动；preserveFocus 保证不会把焦点从讲义/终端等处抢走。
   */
  private async ensureVisible(editor: vscode.TextEditor): Promise<void> {
    const activeUri = vscode.window.activeTextEditor?.document.uri.toString()
    if (
      editor.viewColumn !== undefined &&
      activeUri === editor.document.uri.toString()
    ) {
      return
    }
    await vscode.window.showTextDocument(editor.document, {
      preview: false,
      preserveFocus: true,
      viewColumn: editor.viewColumn ?? vscode.ViewColumn.Active,
    })
  }

  private async readTextFile(uri: vscode.Uri): Promise<string | undefined> {
    try {
      return new TextDecoder().decode(await vscode.workspace.fs.readFile(uri))
    } catch (error) {
      if (
        error instanceof vscode.FileSystemError &&
        error.code === 'FileNotFound'
      ) {
        return undefined
      }
      throw error
    }
  }

  async closeSidePreviews(): Promise<void> {
    pruneOpenedPreviews()
    for (const entry of [...openedPreviews]) {
      const group = vscode.window.tabGroups.all.find((candidate) =>
        candidate.tabs.includes(entry.tab),
      )
      if (!group || group.viewColumn === vscode.ViewColumn.One) {
        continue
      }
      let closed = await vscode.window.tabGroups.close(entry.tab, true)
      if (!closed) {
        closed = await vscode.window.tabGroups.close(entry.tab)
      }
      if (!closed) {
        continue
      }
      const index = openedPreviews.indexOf(entry)
      if (index >= 0) {
        openedPreviews.splice(index, 1)
      }
    }
  }

  async closeFile(filePath: string): Promise<void> {
    const targetPath = await this.safeResolvedPath(filePath)
    const document = vscode.workspace.textDocuments.find((open) =>
      samePath(open.uri.fsPath, targetPath),
    )
    const uriKey =
      document?.uri.toString() ?? vscode.Uri.file(targetPath).toString()
    // 脏 tab 直接关会弹保存确认，ACK 一直等不到；先保存，失败就报错。
    if (document?.isDirty) {
      const saved = await document.save()
      if (!saved && document.isDirty) {
        throw new ApplyError(
          'save_failed',
          `${filePath} could not be saved before closing`,
        )
      }
    }
    for (const { tab } of tabsForFile(
      vscode.window.tabGroups.all,
      targetPath,
    )) {
      let closed = await vscode.window.tabGroups.close(tab, true)
      if (!closed) {
        closed = await vscode.window.tabGroups.close(tab)
      }
      // 并发的用户关闭会让快照中的 tab 已经消失；这仍是幂等成功。
      if (!closed && allTabs().includes(tab)) {
        throw new ApplyError('close_failed', `${filePath} could not be closed`)
      }
    }
    this.editors.delete(uriKey)
    if (this.decoratedEditor?.document.uri.toString() === uriKey) {
      this.decoratedEditor = undefined
    }
  }

  async closePreview(filePath: string): Promise<void> {
    const targetPath = await this.safeResolvedPath(filePath)
    pruneOpenedPreviews()
    for (const entry of [...openedPreviews]) {
      if (!samePath(entry.filePath, targetPath)) {
        continue
      }
      let closed = await vscode.window.tabGroups.close(entry.tab, true)
      if (!closed) {
        closed = await vscode.window.tabGroups.close(entry.tab)
      }
      if (!closed && allTabs().includes(entry.tab)) {
        throw new ApplyError(
          'close_failed',
          `The preview of ${filePath} could not be closed`,
        )
      }
      const index = openedPreviews.indexOf(entry)
      if (index >= 0) {
        openedPreviews.splice(index, 1)
      }
    }
  }

  async preview(filePath: string, view: PreviewView): Promise<void> {
    pruneOpenedPreviews()
    const targetPath = await this.safeResolvedPath(filePath)
    const uri = vscode.Uri.file(targetPath)
    const kind = await this.inspectUri(uri)
    if (kind === 'missing') {
      throw new ApplyError('file_not_found', `${filePath} does not exist`)
    }
    if (kind !== 'file') {
      throw new ApplyError('target_type_conflict', `${filePath} is not a file`)
    }
    // `showPreviewToSide` 只有在已有打开的编辑器时才会真正分栏；没有编辑器时
    // VS Code 会把它放进当前组。要保证并排，课程应先 OpenFile 再预览。
    const command =
      view === 'current' ? 'markdown.showPreview' : 'markdown.showPreviewToSide'
    const before = new Set(allTabs())
    await vscode.commands.executeCommand(command, uri)
    const tab = await locatePreviewTab(before)
    if (tab) {
      const existing = openedPreviews.find((entry) => entry.tab === tab)
      if (existing) {
        // 同一个预览 panel 被复用来显示另一份文档。
        existing.filePath = targetPath
      } else {
        openedPreviews.push({ tab, filePath: targetPath })
      }
    }
  }

  async clearReveal(): Promise<void> {
    this.clearRevealDecoration()
  }

  async reveal(
    target: PreparedTarget,
    start: Position,
    end: Position,
  ): Promise<void> {
    const editor = this.getEditor(target)
    // 后台 tab 必须带到前台，否则 revealRange 和 decoration 都不可见。
    await this.ensureVisible(editor)
    this.clearRevealDecoration()
    const range = new vscode.Range(
      start.line,
      start.character,
      end.line,
      end.character,
    )
    editor.revealRange(
      range,
      vscode.TextEditorRevealType.InCenterIfOutsideViewport,
    )
    editor.setDecorations(this.revealDecorationType(), [range])
    this.decoratedEditor = editor
  }

  private revealDecorationType(): vscode.TextEditorDecorationType {
    this.revealDecoration ??= vscode.window.createTextEditorDecorationType({
      isWholeLine: true,
      backgroundColor: new vscode.ThemeColor(
        'editor.findMatchHighlightBackground',
      ),
      borderColor: new vscode.ThemeColor('editor.findMatchBorder'),
      borderStyle: 'solid',
      borderWidth: '1px',
    })
    return this.revealDecoration
  }

  private clearRevealDecoration(): void {
    if (!this.decoratedEditor) {
      return
    }
    this.decoratedEditor.setDecorations(this.revealDecorationType(), [])
    this.decoratedEditor = undefined
  }

  async open(filePath: string, placement?: 'first'): Promise<PreparedTarget> {
    const targetPath = await this.safeResolvedPath(filePath)
    const uri = vscode.Uri.file(targetPath)
    const kind = await this.inspectUri(uri)
    if (kind === 'missing') {
      throw new ApplyError('file_not_found', `${filePath} does not exist`)
    }
    if (kind !== 'file') {
      throw new ApplyError('target_type_conflict', `${filePath} is not a file`)
    }

    const document = await vscode.workspace.openTextDocument(uri)
    const editor = await vscode.window.showTextDocument(document, {
      preview: false,
      preserveFocus: false,
      ...(placement === 'first' ? { viewColumn: vscode.ViewColumn.One } : {}),
    })
    const diskText = await this.readTextFile(uri)
    if (diskText === undefined) {
      throw new ApplyError('save_failed', 'The target file disappeared')
    }
    this.pruneClosedEditors()
    this.editors.set(document.uri.toString(), editor)
    return {
      uri: document.uri.toString(),
      lineCount: editor.document.lineCount,
      version: editor.document.version,
      savedText: diskText,
      dirty: editor.document.isDirty,
    }
  }

  async existingTarget(filePath: string): Promise<PreparedTarget> {
    const targetPath = await this.safeResolvedPath(filePath)
    const uri = vscode.Uri.file(targetPath)
    const kind = await this.inspectUri(uri)
    if (kind === 'missing') {
      throw new ApplyError('file_not_found', `${filePath} does not exist`)
    }
    if (kind !== 'file') {
      throw new ApplyError('target_type_conflict', `${filePath} is not a file`)
    }
    const target = this.editors.get(uri.toString())
    if (!target || target.document.isClosed) {
      throw new ApplyError(
        'document_not_open',
        `${filePath} must be opened by OpenFile before it is edited`,
      )
    }
    const diskText = await this.readTextFile(uri)
    if (diskText === undefined) {
      throw new ApplyError('save_failed', 'The target file disappeared')
    }
    return {
      uri: uri.toString(),
      lineCount: target.document.lineCount,
      version: target.document.version,
      savedText: diskText,
      dirty: target.document.isDirty,
    }
  }

  async insert(
    target: PreparedTarget,
    position: Position,
    text: string,
    options: InsertOptions,
  ): Promise<void> {
    const editor = this.getEditor(target)
    if (editor.document.version !== target.version) {
      throw new ApplyError(
        'document_version_changed',
        'The target document changed during playback',
      )
    }

    // 若代码文件在后台 tab，先把它的 tab 带到前台（不抢焦点），否则下面的
    // 设光标与 reveal 都不会产生可见效果。
    await this.ensureVisible(editor)
    // 写入前先把光标停在插入点，并把该行带入视口（仅当不在视口内才居中），
    // 让用户在视觉上先看到要改的是哪一行，再观看随后的写入/打字过程。
    editor.selection = new vscode.Selection(
      position.line,
      position.character,
      position.line,
      position.character,
    )
    editor.revealRange(
      new vscode.Range(
        position.line,
        position.character,
        position.line,
        position.character,
      ),
      vscode.TextEditorRevealType.InCenterIfOutsideViewport,
    )
    const applied = await editor.edit((builder) => {
      builder.insert(
        new vscode.Position(position.line, position.character),
        text,
      )
    }, options)
    if (!applied) {
      throw new ApplyError('edit_failed', 'VS Code rejected the edit')
    }
    target.version = editor.document.version
  }

  async matchAt(
    target: PreparedTarget,
    position: Position,
    expected: string,
  ): Promise<TextMatch> {
    const editor = this.getEditor(target)
    const document = editor.document
    const start = new vscode.Position(position.line, position.character)
    const startOffset = document.offsetAt(start)
    const actual = document.getText().slice(startOffset)
    const match = matchTextPrefix(actual, expected)
    const next = document.positionAt(startOffset + match.consumed)

    return {
      matched: match.matched,
      position: {
        line: next.line,
        character: next.character,
      },
    }
  }

  async text(target: PreparedTarget): Promise<string> {
    return this.getEditor(target).document.getText()
  }

  async replace(
    target: PreparedTarget,
    start: Position,
    end: Position,
    text: string,
    options: InsertOptions,
  ): Promise<void> {
    const editor = this.getEditor(target)
    if (editor.document.version !== target.version) {
      throw new ApplyError(
        'document_version_changed',
        'The target document changed during playback',
      )
    }
    // 若代码文件在后台 tab，先把它的 tab 带到前台（不抢焦点），否则下面的
    // 设光标与 reveal 都不会产生可见效果。
    await this.ensureVisible(editor)
    // 写入前先把光标停在替换起点，并把该行带入视口（仅当不在视口内才居中），
    // 让用户在视觉上先看到要改的是哪一行，再观看随后的替换过程。
    editor.selection = new vscode.Selection(
      start.line,
      start.character,
      start.line,
      start.character,
    )
    editor.revealRange(
      new vscode.Range(
        start.line,
        start.character,
        start.line,
        start.character,
      ),
      vscode.TextEditorRevealType.InCenterIfOutsideViewport,
    )
    const applied = await editor.edit((builder) => {
      builder.replace(
        new vscode.Range(start.line, start.character, end.line, end.character),
        text,
      )
    }, options)
    if (!applied) {
      throw new ApplyError('edit_failed', 'VS Code rejected the edit')
    }
    target.version = editor.document.version
  }

  async select(
    target: PreparedTarget,
    start: Position,
    end: Position,
  ): Promise<void> {
    const editor = this.getEditor(target)
    if (editor.document.version !== target.version) {
      throw new ApplyError(
        'document_version_changed',
        'The target document changed during playback',
      )
    }
    // 若代码文件在后台 tab，先把它的 tab 带到前台（不抢焦点），否则下面的
    // 设选区与 reveal 都不会产生可见效果。
    await this.ensureVisible(editor)
    const range = new vscode.Range(
      start.line,
      start.character,
      end.line,
      end.character,
    )
    editor.selection = new vscode.Selection(
      start.line,
      start.character,
      end.line,
      end.character,
    )
    editor.revealRange(
      range,
      vscode.TextEditorRevealType.InCenterIfOutsideViewport,
    )
  }

  async save(target: PreparedTarget): Promise<void> {
    const editor = this.getEditor(target)
    if (editor.document.version !== target.version) {
      throw new ApplyError(
        'document_version_changed',
        'The target document changed during playback',
      )
    }

    const currentText = editor.document.getText()
    const uri = vscode.Uri.parse(target.uri)
    const diskText = await this.readTextFile(uri)
    if (
      !canSafelyWriteDocumentSnapshot(target.savedText, diskText, currentText)
    ) {
      throw new ApplyError(
        'external_file_change',
        'The target file changed outside Cynapse',
      )
    }

    if (editor.document.isDirty) {
      const saved = await editor.document.save()
      if (editor.document.getText() !== currentText) {
        throw new ApplyError(
          'document_version_changed',
          'The target document changed during save',
        )
      }
      if (!saved || editor.document.isDirty) {
        throw new ApplyError(
          'save_failed',
          'VS Code could not save the target file',
        )
      }
    }
    const persistedText = await this.readTextFile(uri)
    if (persistedText === undefined || editor.document.isDirty) {
      throw new ApplyError(
        'save_failed',
        'VS Code could not save the target file',
      )
    }
    // Disk text is UTF-8-decoded only for external-change detection. A
    // non-UTF-8 file can therefore differ from the document text here even
    // though VS Code saved it correctly in its own encoding.
    target.savedText = persistedText
    target.version = editor.document.version
  }

  async endUndoGroup(target: PreparedTarget): Promise<void> {
    const editor = this.getEditor(target)
    const applied = await editor.edit(() => {}, {
      undoStopBefore: false,
      undoStopAfter: true,
    })
    if (!applied) {
      throw new ApplyError('edit_failed', 'VS Code rejected the undo boundary')
    }
  }

  async inspect(requestedPath: string): Promise<TargetKind> {
    return this.inspectUri(
      vscode.Uri.file(this.resolveRequestedPath(requestedPath)),
    )
  }

  async createDirectory(
    requestedPath: string,
    createParents: boolean,
  ): Promise<void> {
    const uri = vscode.Uri.file(await this.safeResolvedPath(requestedPath))
    if (createParents) {
      const parent = path.dirname(uri.fsPath)
      if (parent !== this.workspaceRoot()) {
        await this.ensureDirectory(vscode.Uri.file(parent), true)
      }
    }
    await vscode.workspace.fs.createDirectory(uri)
  }

  async createFile(
    requestedPath: string,
    createParents: boolean,
  ): Promise<void> {
    const uri = vscode.Uri.file(await this.safeResolvedPath(requestedPath))
    if (createParents) {
      const parent = path.dirname(uri.fsPath)
      if (parent !== this.workspaceRoot()) {
        await this.ensureDirectory(vscode.Uri.file(parent), true)
      }
    }
    await vscode.workspace.fs.writeFile(uri, new Uint8Array())
  }

  async deleteFile(requestedPath: string): Promise<void> {
    const uri = vscode.Uri.file(await this.safeResolvedPath(requestedPath))
    await vscode.workspace.fs.delete(uri, {
      recursive: false,
      useTrash: true,
    })
  }

  async directoryIsEmpty(requestedPath: string): Promise<boolean> {
    const uri = vscode.Uri.file(await this.safeResolvedPath(requestedPath))
    const entries = await vscode.workspace.fs.readDirectory(uri)
    return entries.length === 0
  }

  async deleteDirectory(
    requestedPath: string,
    recursive: boolean,
  ): Promise<void> {
    const uri = vscode.Uri.file(await this.safeResolvedPath(requestedPath))
    await vscode.workspace.fs.delete(uri, {
      recursive,
      useTrash: true,
    })
  }

  async confirmRecursiveDelete(requestedPath: string): Promise<boolean> {
    const deleteLabel = t('Delete')
    const choice = await vscode.window.showWarningMessage(
      t(
        'Cynapse will recursively delete "{0}" from the workspace.',
        requestedPath,
      ),
      { modal: true },
      deleteLabel,
    )
    return choice === deleteLabel
  }

  async delay(ms: number, signal?: AbortSignal): Promise<void> {
    if (signal?.aborted) {
      return
    }
    await new Promise<void>((resolve) => {
      const finish = () => {
        clearTimeout(timer)
        signal?.removeEventListener('abort', finish)
        resolve()
      }
      const timer = setTimeout(finish, ms)
      signal?.addEventListener('abort', finish, { once: true })
    })
  }

  private async inspectUri(uri: vscode.Uri): Promise<TargetKind> {
    try {
      const stat = await vscode.workspace.fs.stat(uri)
      if ((stat.type & vscode.FileType.SymbolicLink) !== 0) {
        throw new ApplyError(
          'unsupported_target_type',
          `${uri.fsPath} is a symbolic link`,
        )
      }
      if ((stat.type & vscode.FileType.Directory) !== 0) {
        return 'directory'
      }
      if ((stat.type & vscode.FileType.File) !== 0) {
        return 'file'
      }
      throw new ApplyError(
        'unsupported_target_type',
        `${uri.fsPath} is not a regular file or directory`,
      )
    } catch (error) {
      if (
        error instanceof vscode.FileSystemError &&
        error.code === 'FileNotFound'
      ) {
        return 'missing'
      }
      throw error
    }
  }

  private async ensureDirectory(
    uri: vscode.Uri,
    createParents: boolean,
  ): Promise<void> {
    const kind = await this.inspectUri(uri)
    if (kind === 'directory') {
      return
    }
    if (kind !== 'missing') {
      throw new ApplyError(
        'target_type_conflict',
        `${uri.fsPath} is not a directory`,
      )
    }
    if (createParents && uri.fsPath !== this.workspaceRoot()) {
      const parent = path.dirname(uri.fsPath)
      if (parent !== this.workspaceRoot()) {
        await this.ensureDirectory(vscode.Uri.file(parent), true)
      }
    }
    await vscode.workspace.fs.createDirectory(uri)
  }
}

export function createVSCodeFacade(workspaceRoot?: string): VSCodeFacade {
  return new VSCodeFacade(workspaceRoot)
}
