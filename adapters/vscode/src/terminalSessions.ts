import * as path from 'node:path'

import {
  directoryIsWithin,
  resolveRealWorkspacePath,
  samePath,
} from './workspacePaths'

export interface TerminalSessionHost<Terminal> {
  create(sessionKey: string, cwdPath: string): Terminal
  isClosed(terminal: Terminal): boolean
  dispose(terminal: Terminal): void
  show(terminal: Terminal, preserveFocus: boolean): void
  waitUntilReady(terminal: Terminal): Promise<void>
}

export interface TerminalSessionTarget {
  sessionKey: string
  workspaceRoot: string
  cwdPath: string
  preserveFocus: boolean
  newTerminal?: boolean
}

export interface TerminalSessionEntry<Terminal> {
  terminal: Terminal
  workspaceRoot: string
  cwdPath: string
}

const CYNAPSE_TERMINAL_PREFIX = 'Cynapse · '

export interface TerminalOwnership {
  name: string
  cwdPath?: string
}

// A terminal left over from a previous extension host belongs to this window
// only when both its name and its creation directory say so.
export function terminalBelongsToWorkspace(
  terminal: TerminalOwnership,
  workspaceRoot: string,
): boolean {
  return (
    terminal.name.startsWith(CYNAPSE_TERMINAL_PREFIX) &&
    terminal.cwdPath !== undefined &&
    directoryIsWithin(workspaceRoot, terminal.cwdPath)
  )
}

// Tracked terminals are disposed by ownership alone; untracked ones need the
// Cynapse name, so a user terminal that happens to sit in a reset path stays.
export function terminalsToDisposeUnderPaths<T extends TerminalOwnership>(
  tracked: Iterable<T>,
  allTerminals: Iterable<T>,
  workspaceRoot: string,
  resetPaths: readonly string[],
  excludePaths: readonly string[] = [],
): T[] {
  const inExcludedScope = (cwdPath: string | undefined): boolean =>
    cwdPath !== undefined &&
    excludePaths.some((excludePath) =>
      directoryIsWithin(path.resolve(workspaceRoot, excludePath), cwdPath),
    )
  const inResetScope = (terminal: T): boolean =>
    (resetPaths.length === 0 ||
      (terminal.cwdPath !== undefined &&
        terminalCwdIsWithinResetPaths(
          workspaceRoot,
          resetPaths,
          terminal.cwdPath,
        ))) &&
    !inExcludedScope(terminal.cwdPath)
  const managed = new Set([...tracked].filter(inResetScope))
  for (const terminal of allTerminals) {
    if (
      terminalBelongsToWorkspace(terminal, workspaceRoot) &&
      inResetScope(terminal)
    ) {
      managed.add(terminal)
    }
  }
  return [...managed]
}

// Session-scoped cleanup names the sessions to close; path scope is ignored so
// an environment check cannot reach terminals other Units left open.
export function filterSessionEntries<T>(
  entries: Iterable<[string, T]>,
  sessionKeys: readonly string[],
): Array<[string, T]> {
  const wanted = new Set(sessionKeys)
  return [...entries].filter(([sessionKey]) => wanted.has(sessionKey))
}

export function terminalCwdIsWithinResetPaths(
  workspaceRoot: string,
  resetPaths: readonly string[],
  terminalCwd: string,
): boolean {
  return resetPaths.some((resetPath) =>
    directoryIsWithin(path.resolve(workspaceRoot, resetPath), terminalCwd),
  )
}

// Resolves `cwd` against the workspace root through the real filesystem so a
// symlinked directory cannot escape the workspace.
export function resolveTerminalDirectory(
  workspaceRoot: string,
  cwd: string,
): Promise<string> {
  return resolveRealWorkspacePath(
    workspaceRoot,
    path.resolve(workspaceRoot, cwd),
  )
}

export function terminalDirectoryCommand(
  shell: string | undefined,
  cwdPath: string,
): string {
  switch (shell) {
    case 'pwsh':
      return `cd -- '${escapePowerShell(cwdPath)}'`
    case 'cmd':
      return `cd /d "${escapeDoubleQuoted(cwdPath)}"`
    case 'fish':
      return `cd '${escapeSingleQuoted(cwdPath)}'`
    case 'bash':
    case 'csh':
    case 'gitbash':
    case 'ksh':
    case 'sh':
    case 'wsl':
    case 'zsh':
      return `cd -- '${escapeSingleQuoted(cwdPath)}'`
    default:
      return `cd "${escapeDoubleQuoted(cwdPath)}"`
  }
}

export function sameTerminalDirectory(
  left: string | undefined,
  right: string,
): boolean {
  if (!left) {
    return false
  }
  const normalizedLeft = normalizeTerminalPath(left)
  const normalizedRight = normalizeTerminalPath(right)
  return samePath(normalizedLeft, normalizedRight)
}

export class TerminalSessionManager<Terminal> {
  private readonly sessions = new Map<string, TerminalSessionEntry<Terminal>>()

  constructor(private readonly host: TerminalSessionHost<Terminal>) {}

  async ensure(
    target: TerminalSessionTarget,
  ): Promise<{ entry: TerminalSessionEntry<Terminal>; created: boolean }> {
    let entry = this.sessions.get(target.sessionKey)
    let created = false
    if (
      !entry ||
      entry.workspaceRoot !== target.workspaceRoot ||
      target.newTerminal === true ||
      this.host.isClosed(entry.terminal)
    ) {
      if (entry) {
        this.host.dispose(entry.terminal)
      }
      entry = {
        workspaceRoot: target.workspaceRoot,
        cwdPath: target.cwdPath,
        terminal: this.host.create(target.sessionKey, target.cwdPath),
      }
      this.sessions.set(target.sessionKey, entry)
      created = true
    } else {
      entry.cwdPath = target.cwdPath
    }

    this.host.show(entry.terminal, target.preserveFocus)
    await this.host.waitUntilReady(entry.terminal)
    return { entry, created }
  }

  get(sessionKey: string): TerminalSessionEntry<Terminal> | undefined {
    return this.sessions.get(sessionKey)
  }

  remove(sessionKey: string): TerminalSessionEntry<Terminal> | undefined {
    const entry = this.sessions.get(sessionKey)
    this.sessions.delete(sessionKey)
    return entry
  }

  entries(): Array<[string, TerminalSessionEntry<Terminal>]> {
    return [...this.sessions.entries()]
  }
}

function normalizeTerminalPath(value: string): string {
  if (process.platform === 'win32') {
    return value.replace(/\//g, '\\').replace(/\\+$/g, '')
  }
  return value.replace(/\/+$/g, '')
}

function escapeSingleQuoted(value: string): string {
  return value.replace(/'/g, `'\\''`)
}

function escapePowerShell(value: string): string {
  return value.replace(/'/g, `''`)
}

function escapeDoubleQuoted(value: string): string {
  return value.replace(/"/g, '\\"')
}
