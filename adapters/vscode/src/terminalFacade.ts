import { rm } from 'node:fs/promises'
import * as path from 'node:path'
import * as vscode from 'vscode'

import { ApplyError } from './apply'
import { resolveWorkspacePath } from './filesystem'
import type { TerminalCommandAction } from '@cynapse/adapter-kit'
import {
  awaitInterruptibleExecution,
  collectOutput,
  executeTerminalCommandInDirectory,
  groupCompoundShellCommand,
  MAX_TERMINAL_OUTPUT_BYTES,
  readBoundedFileTail,
  redirectShellOutput,
  shellIntegrationUnavailable,
  type TerminalExecutionResult,
  type TerminalFacade,
} from './terminal'
import {
  filterSessionEntries,
  resolveTerminalDirectory,
  sameTerminalDirectory,
  TerminalSessionManager,
  terminalCwdIsWithinResetPaths,
  terminalsToDisposeUnderPaths,
} from './terminalSessions'
import { workspaceRootFromConfig } from './workspace'

export function createVSCodeTerminalFacade(
  configuredWorkspaceRoot?: string,
): TerminalFacade {
  return new VSCodeTerminalFacade(configuredWorkspaceRoot)
}

let captureSequence = 0
const terminalSessions = new TerminalSessionManager<vscode.Terminal>({
  create(sessionKey, cwdPath) {
    return vscode.window.createTerminal({
      name: `Cynapse · ${sessionKey}`,
      cwd: vscode.Uri.file(cwdPath),
      isTransient: true,
    })
  },
  isClosed(terminal) {
    return terminal.exitStatus !== undefined
  },
  dispose(terminal) {
    terminal.dispose()
  },
  show(terminal, preserveFocus) {
    terminal.show(preserveFocus)
  },
  waitUntilReady(terminal) {
    return waitForShellIntegration(terminal)
  },
})

class VSCodeTerminalFacade implements TerminalFacade {
  constructor(private readonly configuredWorkspaceRoot?: string) {}

  async ensure(
    sessionKey: string,
    cwd: string,
    preserveFocus: boolean,
    newTerminal = false,
  ): Promise<void> {
    const workspaceRoot = this.workspaceRoot()
    const cwdPath = await resolveTerminalDirectory(workspaceRoot, cwd)
    await terminalSessions.ensure({
      sessionKey,
      workspaceRoot,
      cwdPath,
      preserveFocus,
      newTerminal,
    })
  }

  async completionMarkerExists(completionMarker: string): Promise<boolean> {
    const markerPath = resolveWorkspacePath(
      this.workspaceRoot(),
      completionMarker,
    )
    try {
      const stat = await vscode.workspace.fs.stat(vscode.Uri.file(markerPath))
      if ((stat.type & vscode.FileType.File) === 0) {
        throw new ApplyError(
          'completion_marker_not_file',
          `Completion marker ${completionMarker} is not a file`,
        )
      }
      return true
    } catch (error) {
      if (
        error instanceof vscode.FileSystemError &&
        error.code === 'FileNotFound'
      ) {
        return false
      }
      throw error
    }
  }

  async disposeUnderPaths(
    paths: string[],
    excludePaths: string[] = [],
    sessionKeys: readonly string[] = [],
  ): Promise<void> {
    const workspaceRoot = this.workspaceRoot()
    const entries = terminalSessions
      .entries()
      .filter(([, entry]) =>
        sameTerminalDirectory(entry.workspaceRoot, workspaceRoot),
      )
    const tracked = entries.map(([, entry]) => ({
      terminal: entry.terminal,
      name: entry.terminal.name,
      cwdPath: entry.cwdPath,
    }))
    const allTerminals = vscode.window.terminals.map((terminal) => ({
      terminal,
      name: terminal.name,
      cwdPath: terminalCreationDirectory(terminal),
    }))
    // Session scope wins over path scope: an environment check closes exactly
    // the terminal it created, never the ones other Units left open.
    const bySession = sessionKeys.length > 0
    const matchingEntries = bySession
      ? filterSessionEntries(entries, sessionKeys)
      : paths.length === 0
        ? entries
        : entries.filter(([, entry]) =>
            terminalCwdIsWithinResetPaths(workspaceRoot, paths, entry.cwdPath),
          )
    const matching = bySession
      ? matchingEntries.map(([, entry]) => entry.terminal)
      : [
          ...new Set(
            terminalsToDisposeUnderPaths(
              tracked,
              allTerminals,
              workspaceRoot,
              paths,
              excludePaths,
            ).map(({ terminal }) => terminal),
          ),
        ]
    const results = await Promise.allSettled(
      matching.map((terminal) => waitForTerminalToClose(terminal)),
    )
    const failure = results.find((result) => result.status === 'rejected')
    if (failure?.status === 'rejected') {
      throw failure.reason
    }
    for (const [sessionKey, entry] of matchingEntries) {
      if (matching.includes(entry.terminal)) {
        terminalSessions.remove(sessionKey)
      }
    }
  }

  async execute(
    action: TerminalCommandAction,
    signal?: AbortSignal,
  ): Promise<TerminalExecutionResult> {
    if (signal?.aborted) {
      return {
        exitCode: undefined,
        output: '',
        cancelled: true,
        timedOut: false,
      }
    }

    await this.ensure(
      action.session_key,
      action.cwd,
      true,
      action.new_terminal ?? false,
    )
    const entry = terminalSessions.get(action.session_key)
    const terminal = entry?.terminal
    const shellIntegration = terminal?.shellIntegration
    if (!terminal || !shellIntegration) {
      throw shellIntegrationUnavailable()
    }
    const shell = terminal.state.shell
    const capturePath = action.capture_to_file
      ? path.join(
          entry.cwdPath,
          `.cynapse-check-${process.pid}-${captureSequence++}.log`,
        )
      : undefined
    try {
      const result = await executeTerminalCommandInDirectory({
        currentDirectory: shellIntegration.cwd?.fsPath,
        targetDirectory: entry.cwdPath,
        shell,
        command: action.command,
        timeoutMs: action.timeout_ms,
        execute: (command, timeoutMs) =>
          executeShellCommand(
            terminal,
            shellIntegration,
            command,
            timeoutMs,
            signal,
            shell,
            capturePath && command === action.command ? capturePath : undefined,
          ),
      })
      if (capturePath) {
        try {
          const tail = await readBoundedFileTail(
            capturePath,
            MAX_TERMINAL_OUTPUT_BYTES,
          )
          result.output = tail.output
          result.outputTruncated = tail.truncated
        } catch {
          // Leave the terminal-captured output when the shell did not write
          // the redirection file (e.g. a directory-change failure).
        }
      }
      if (result.timedOut || result.cancelled) {
        const stale = terminalSessions.remove(action.session_key)
        stale?.terminal.dispose()
      }
      return result
    } finally {
      if (capturePath) {
        await rm(capturePath, { force: true }).catch(() => undefined)
      }
    }
  }

  private workspaceRoot(): string {
    const root = workspaceRootFromConfig(this.configuredWorkspaceRoot)
    if (!root) {
      throw new ApplyError(
        'workspace_root_missing',
        'Open a course workspace before running a terminal command',
      )
    }
    return root
  }
}

async function executeShellCommand(
  terminal: vscode.Terminal,
  shellIntegration: vscode.TerminalShellIntegration,
  command: string,
  timeoutMs: number,
  signal?: AbortSignal,
  shell?: string,
  capturePath?: string,
): Promise<TerminalExecutionResult> {
  if (signal?.aborted) {
    return {
      exitCode: undefined,
      output: '',
      cancelled: true,
      timedOut: false,
    }
  }

  let execution: vscode.TerminalShellExecution | undefined
  let resolveExit!: (exitCode: number | undefined) => void
  const exitCode = new Promise<number | undefined>((resolve) => {
    resolveExit = resolve
  })
  const exitListener = vscode.window.onDidEndTerminalShellExecution((event) => {
    if (event.execution === execution) {
      resolveExit(event.exitCode)
    }
  })

  const grouped = groupCompoundShellCommand(command, shell)
  execution = shellIntegration.executeCommand(
    capturePath ? redirectShellOutput(grouped, shell, capturePath) : grouped,
  )
  const outputPromise = collectOutput(execution.read())
  const completion = Promise.all([exitCode, outputPromise]).then(
    ([code, output]) => ({ code, output }),
  )

  try {
    return await awaitInterruptibleExecution(
      {
        completion: completion.then(({ code, output }) => ({
          exitCode: code,
          output,
          outputTruncated: outputPromise.truncated(),
        })),
        partialOutput: outputPromise.partial,
        partialOutputTruncated: outputPromise.truncated,
        interrupt: () => terminal.sendText('\u0003', false),
        timeoutMs,
      },
      signal,
    )
  } finally {
    exitListener.dispose()
  }
}

function terminalCreationDirectory(
  terminal: vscode.Terminal,
): string | undefined {
  const options = terminal.creationOptions
  if (!('cwd' in options) || options.cwd === undefined) {
    return undefined
  }
  return typeof options.cwd === 'string' ? options.cwd : options.cwd.fsPath
}

async function waitForShellIntegration(
  terminal: vscode.Terminal,
): Promise<void> {
  if (terminal.shellIntegration) {
    return
  }

  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      listener.dispose()
      reject(shellIntegrationUnavailable())
    }, 5_000)
    const listener = vscode.window.onDidChangeTerminalShellIntegration(
      (event) => {
        if (event.terminal === terminal) {
          clearTimeout(timeout)
          listener.dispose()
          resolve()
        }
      },
    )
    if (terminal.shellIntegration) {
      clearTimeout(timeout)
      listener.dispose()
      resolve()
    }
  })
}

async function waitForTerminalToClose(
  terminal: vscode.Terminal,
): Promise<void> {
  if (terminal.exitStatus !== undefined) {
    return
  }
  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      listener.dispose()
      reject(
        new ApplyError(
          'terminal_dispose_timeout',
          'Cynapse terminal did not close before workspace reset',
        ),
      )
    }, 2_000)
    const listener = vscode.window.onDidCloseTerminal((closed) => {
      if (closed === terminal) {
        clearTimeout(timeout)
        listener.dispose()
        resolve()
      }
    })
    terminal.dispose()
  })
}
