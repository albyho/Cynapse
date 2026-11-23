import * as path from 'node:path'
import * as vscode from 'vscode'

import { parseCheckPayload } from '@cynapse/adapter-kit'

import { applyAction, parseCourseAction, writeActionPath } from './actions'
import { ApplyError } from './apply'
import { dirtyDocumentsUnder, resolveUnitCheckCwd } from './check'
import {
  resolveMutableUnitWorkspacePath,
  resolveTerminalWorkspacePath,
  resolveUnitWorkspacePath,
} from './filesystem'
import {
  VSCodeAdapterConnection,
  type ActionHandler,
  type CheckHandler,
  type VSCodeAdapterStatus,
} from './adapterConnection'
import { resolveDiscovery } from './discovery'
import { tabsForWorkspaceReset } from './tabs'
import { applyTerminalAction } from './terminal'
import { createVSCodeTerminalFacade } from './terminalFacade'
import {
  createVSCodeFacade,
  openedPreviews,
  pruneOpenedPreviews,
} from './vscodeFacade'
import { shouldOpenWorkspace, workspaceRootFromConfig } from './workspace'
import { t } from './i18n'

let activeConnection: VSCodeAdapterConnection | undefined
let pollTimer: NodeJS.Timeout | undefined
let activeSignature = ''

async function openCourseWorkspace(
  targetWorkspace: string | undefined,
): Promise<void> {
  const currentWorkspace = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
  if (!shouldOpenWorkspace(currentWorkspace, targetWorkspace)) {
    return
  }

  await vscode.commands.executeCommand(
    'vscode.openFolder',
    vscode.Uri.file(targetWorkspace!),
    { forceNewWindow: false, forceReuseWindow: true },
  )
}

export function activate(context: vscode.ExtensionContext): void {
  const statusBar = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100,
  )
  statusBar.show()
  context.subscriptions.push(statusBar)

  // 工作区重置涉及删除文件，留下可核对的诊断信息，便于定位 tab 未关闭等问题。
  const resetLog = vscode.window.createOutputChannel(
    t('Cynapse Workspace Reset'),
  )
  context.subscriptions.push(resetLog)
  context.subscriptions.push(
    vscode.window.tabGroups.onDidChangeTabs(() => pruneOpenedPreviews()),
  )
  resetLog.appendLine(
    t(
      'Cynapse VS Code Adapter v{0} activated',
      String(context.extension.packageJSON?.version ?? '?'),
    ),
  )

  let currentStatus = 'Cynapse VS Code Adapter'
  let activeWorkspace: string | undefined
  let terminalFacade = createVSCodeTerminalFacade(activeWorkspace)
  let editorFacade = createVSCodeFacade(activeWorkspace)
  const handleAction: ActionHandler = async (envelope, { signal }) => {
    const action = parseCourseAction(envelope.payload)
    if (!action) {
      const payloadType =
        typeof envelope.payload === 'object' &&
        envelope.payload !== null &&
        'type' in envelope.payload &&
        typeof (envelope.payload as { type?: unknown }).type === 'string'
          ? (envelope.payload as { type: string }).type
          : 'unknown'
      return {
        status: 'error',
        code: 'invalid_action',
        message: t('Invalid course action payload: {0}', payloadType),
      }
    }

    try {
      const writePath = writeActionPath(action)
      if (writePath) {
        const workspace = workspaceRootFromConfig(activeWorkspace)
        if (!workspace) {
          throw new ApplyError(
            'workspace_root_missing',
            'Open a course workspace before applying a file action',
          )
        }
        if (!envelope.unit_path) {
          throw new ApplyError(
            'invalid_action',
            'The course action is missing its unit path',
          )
        }
        await resolveMutableUnitWorkspacePath(
          workspace,
          envelope.unit_path,
          writePath,
        )
      }
      if (
        action.type === 'TerminalEnsure' ||
        action.type === 'TerminalCommand'
      ) {
        if (!envelope.unit_path) {
          throw new ApplyError(
            'invalid_action',
            'The course action is missing its unit path',
          )
        }
        const workspace = workspaceRootFromConfig(activeWorkspace)
        if (!workspace) {
          throw new ApplyError(
            'workspace_root_missing',
            'Open a course workspace before applying a terminal action',
          )
        }
        await resolveTerminalWorkspacePath(
          workspace,
          envelope.unit_path,
          action.cwd,
        )
        if (action.type === 'TerminalCommand' && action.completion_marker) {
          await resolveUnitWorkspacePath(
            workspace,
            envelope.unit_path,
            action.completion_marker,
          )
        }
        return await applyTerminalAction(action, terminalFacade, signal)
      }
      const outcome = await applyAction(
        action,
        { editor: editorFacade, filesystem: editorFacade },
        signal,
      )
      return { status: outcome }
    } catch (error) {
      return {
        status: 'error',
        code: error instanceof ApplyError ? error.code : 'apply_failed',
        message: error instanceof Error ? error.message : String(error),
      }
    }
  }

  const handleCheck: CheckHandler = async (envelope, { signal }) => {
    const check = parseCheckPayload(envelope.payload)
    if (!check) {
      return {
        status: 'error',
        code: 'invalid_check',
        message: t('Invalid challenge check parameters'),
      }
    }

    try {
      const workspace = workspaceRootFromConfig(activeWorkspace)
      if (!workspace) {
        return {
          status: 'error',
          code: 'workspace_root_missing',
          message: 'Open a course workspace before running a challenge check',
        }
      }
      const root = path.resolve(workspace)
      const unitRoot = path.resolve(root, check.unit_path)
      if (unitRoot !== root && !unitRoot.startsWith(`${root}${path.sep}`)) {
        return {
          status: 'error',
          code: 'invalid_check',
          message: t('The challenge unit path escapes the course workspace'),
        }
      }
      const effectiveCwd = resolveUnitCheckCwd(unitRoot, check.cwd)
      if (!effectiveCwd) {
        return {
          status: 'error',
          code: 'invalid_check',
          message: t(
            'The challenge check directory escapes the unit workspace',
          ),
        }
      }
      const dirty = dirtyDocumentsUnder(
        unitRoot,
        vscode.workspace.textDocuments,
      )
      if (dirty.length > 0) {
        if (!check.save_dirty) {
          return {
            status: 'error',
            code: 'workspace_dirty',
            message: t('Save unsaved files before running the challenge check'),
          }
        }
        const saved = await Promise.all(
          dirty.map((document) => document.save()),
        )
        if (saved.some((ok) => !ok)) {
          return {
            status: 'error',
            code: 'check_save_failed',
            message: t(
              'Could not save unsaved files; the challenge check was not run',
            ),
          }
        }
      }
      return await applyTerminalAction(
        {
          type: 'TerminalCommand',
          session_key: check.session_key,
          cwd: effectiveCwd,
          command: check.command,
          accepted_exit_codes: check.accepted_exit_codes,
          expected_output: check.expected_output,
          timeout_ms: check.timeout_ms,
          capture_to_file: true,
        },
        terminalFacade,
        signal,
      )
    } catch (error) {
      return {
        status: 'error',
        code: error instanceof ApplyError ? error.code : 'check_failed',
        message: error instanceof Error ? error.message : String(error),
      }
    }
  }

  const setStatus = (status: VSCodeAdapterStatus): void => {
    if (status.kind === 'connected') {
      statusBar.text = t('$(plug) Cynapse: Connected')
      currentStatus = t(
        'Cynapse VS Code Adapter connected to {0}',
        status.adapter,
      )
    } else if (status.kind === 'error') {
      statusBar.text = t('$(warning) Cynapse: Error')
      currentStatus = t('Cynapse VS Code Adapter error: {0}', status.message)
    } else {
      statusBar.text = t('$(plug) Cynapse: Waiting')
      currentStatus = t('Cynapse VS Code Adapter: {0}', status.message)
    }
    statusBar.tooltip = currentStatus
  }

  const handleWorkspaceReset = async ({
    paths,
    exclude_paths = [],
    session_keys = [],
    close_tabs = true,
  }: {
    paths: string[]
    exclude_paths?: string[]
    session_keys?: string[]
    close_tabs?: boolean
  }): Promise<
    { status: 'done' } | { status: 'error'; code: string; message: string }
  > => {
    if (!activeWorkspace) {
      resetLog.appendLine(
        t('Workspace root is unavailable; cannot reset the course workspace'),
      )
      return {
        status: 'error',
        code: 'workspace_root_missing',
        message: t(
          'Workspace root is unavailable; cannot reset the course workspace',
        ),
      }
    }
    try {
      // Windows shells retain handles on their cwd until disposal completes.
      await terminalFacade.disposeUnderPaths(paths, exclude_paths, session_keys)
      if (close_tabs) {
        // Markdown 预览 tab 是 TabInputWebview（没有 uri）。按打开时记下的
        // tab 对象引用精确匹配，标题完全不参与，避免同名 INTRO.md 串台。
        const describeTab = (tab: vscode.Tab): string => {
          const input = tab.input as
            { viewType?: unknown; uri?: { fsPath?: string } } | undefined
          const viewType =
            typeof input?.viewType === 'string' ? input.viewType : 'no-viewType'
          const uri =
            typeof input?.uri?.fsPath === 'string' ? ` ${input.uri.fsPath}` : ''
          return `${viewType}${uri} active=${tab.isActive}`
        }
        resetLog.appendLine(
          `workspace_reset paths=${JSON.stringify(paths)} exclude_paths=${JSON.stringify(exclude_paths)} session_keys=${JSON.stringify(session_keys)} close_tabs=${close_tabs}`,
        )
        resetLog.appendLine(
          t(
            'Recorded previews={0}',
            JSON.stringify(openedPreviews.map(({ filePath }) => filePath)),
          ),
        )
        resetLog.appendLine(
          t(
            'Current tabs={0}',
            JSON.stringify(
              vscode.window.tabGroups.all.flatMap((group) =>
                group.tabs.map(describeTab),
              ),
            ),
          ),
        )
        const tabs = tabsForWorkspaceReset<vscode.Tab, vscode.TabGroup>(
          activeWorkspace,
          paths,
          vscode.window.tabGroups.all,
          openedPreviews,
          exclude_paths,
        )
        for (const { tab, group } of tabs) {
          let document: vscode.TextDocument | undefined
          try {
            if (tab.input instanceof vscode.TabInputText) {
              const uri = tab.input.uri
              // 文件已删除时 openTextDocument 可能失败；缓冲区还开着就直接复用。
              document =
                vscode.workspace.textDocuments.find(
                  (open) => open.uri.toString() === uri.toString(),
                ) ?? (await vscode.workspace.openTextDocument(uri))
              // `revertAndCloseActiveEditor` targets the active group, so this
              // must activate the tab's group; Reset is already modal.
              await vscode.window.showTextDocument(document, {
                preview: false,
                preserveFocus: false,
                viewColumn: group.viewColumn,
              })
              await vscode.commands.executeCommand(
                'workbench.action.revertAndCloseActiveEditor',
              )
            } else {
              const closed = await vscode.window.tabGroups.close(tab, true)
              if (!closed) {
                resetLog.appendLine(
                  t('First close failed, retrying: {0}', describeTab(tab)),
                )
                await vscode.window.tabGroups.close(tab)
              }
            }
          } catch (error) {
            // 单个 tab 关闭失败不能中断整批，否则后面的 tab 永远轮不到。
            resetLog.appendLine(
              t(
                'Close failed, closing directly: {0} {1}',
                describeTab(tab),
                error instanceof Error ? error.message : String(error),
              ),
            )
            if (document?.isDirty) {
              // 脏文本 tab 的关闭会弹保存确认，可能一直卡到清理超时。
              resetLog.appendLine(
                t(
                  'Buffer is dirty, leaving it for the next cleanup: {0}',
                  describeTab(tab),
                ),
              )
              continue
            }
            try {
              const closed = await vscode.window.tabGroups.close(tab, true)
              if (!closed) {
                resetLog.appendLine(
                  t('Direct close failed: {0}', describeTab(tab)),
                )
              }
            } catch (closeError) {
              resetLog.appendLine(
                t(
                  'Direct close threw: {0} {1}',
                  describeTab(tab),
                  closeError instanceof Error
                    ? closeError.message
                    : String(closeError),
                ),
              )
            }
          }
        }
        const remaining = tabsForWorkspaceReset<vscode.Tab, vscode.TabGroup>(
          activeWorkspace,
          paths,
          vscode.window.tabGroups.all,
          openedPreviews,
          exclude_paths,
        )
        resetLog.appendLine(
          t(
            'Matched {0} tabs, {1} remaining after close{2}',
            tabs.length,
            remaining.length,
            remaining.length > 0
              ? `: ${remaining.map(({ tab }) => describeTab(tab)).join('; ')}`
              : '',
          ),
        )
        if (remaining.length > 0) {
          return {
            status: 'error',
            code: 'workspace_reset_incomplete',
            message: t('Some tabs for deleted files could not be closed'),
          }
        }
        const closedTabs = new Set(tabs.map(({ tab }) => tab))
        for (let index = openedPreviews.length - 1; index >= 0; index -= 1) {
          if (closedTabs.has(openedPreviews[index]!.tab)) {
            openedPreviews.splice(index, 1)
          }
        }
      }
      return { status: 'done' }
    } catch (error) {
      return {
        status: 'error',
        code:
          error instanceof ApplyError ? error.code : 'workspace_reset_failed',
        message: error instanceof Error ? error.message : String(error),
      }
    }
  }

  const check = (): void => {
    const discovery = resolveDiscovery()
    if (!discovery) {
      activeConnection?.dispose()
      activeConnection = undefined
      activeSignature = ''
      setStatus({ kind: 'waiting', message: t('Waiting for Cynapse Client') })
      return
    }

    const signature = `${discovery.pid}:${discovery.port}:${discovery.token}`
    if (signature === activeSignature) {
      return
    }

    // Dispose the old connection before swapping shared workspace/facades so a
    // still-running old handler cannot observe the new workspace.
    activeConnection?.dispose()
    activeWorkspace = discovery.workspace?.trim() || undefined
    terminalFacade = createVSCodeTerminalFacade(activeWorkspace)
    editorFacade = createVSCodeFacade(activeWorkspace)
    activeConnection = new VSCodeAdapterConnection(
      discovery,
      setStatus,
      handleAction,
      handleCheck,
      handleWorkspaceReset,
    )
    activeSignature = signature
    activeConnection.connect()
    void openCourseWorkspace(activeWorkspace).catch((error) => {
      setStatus({
        kind: 'error',
        message: t(
          'Could not open the course workspace: {0}',
          error instanceof Error ? error.message : String(error),
        ),
      })
    })
  }

  check()
  pollTimer = setInterval(check, 1000)
  context.subscriptions.push(
    new vscode.Disposable(() => {
      if (pollTimer) {
        clearInterval(pollTimer)
        pollTimer = undefined
      }
    }),
  )

  context.subscriptions.push(
    vscode.commands.registerCommand('cynapse.showStatus', () => {
      void vscode.window.showInformationMessage(currentStatus)
    }),
  )
}

export function deactivate(): void {
  if (pollTimer) {
    clearInterval(pollTimer)
    pollTimer = undefined
  }
  activeConnection?.dispose()
  activeConnection = undefined
  activeSignature = ''
}
