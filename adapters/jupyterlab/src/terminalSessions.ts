export interface TerminalSessionHost<Terminal> {
  create(sessionKey: string, cwdPath: string): Promise<Terminal>
  isClosed(terminal: Terminal): boolean
  dispose(terminal: Terminal): Promise<void>
}

export interface TerminalSession {
  sessionKey: string
  cwdPath: string
}

export interface TerminalSessionEntry<Terminal> {
  terminal: Terminal
  cwdPath: string
}

/**
 * One Cynapse Terminal Session per `session_key`. The working directory is part
 * of the session identity: a missing, closed, or moved session is recreated
 * before the next command runs.
 */
export class TerminalSessionManager<Terminal> {
  private readonly sessions = new Map<string, TerminalSessionEntry<Terminal>>()

  constructor(private readonly host: TerminalSessionHost<Terminal>) {}

  async ensure(
    sessionKey: string,
    cwdPath: string,
    newTerminal = false,
  ): Promise<TerminalSessionEntry<Terminal>> {
    const entry = this.sessions.get(sessionKey)
    if (
      entry &&
      !newTerminal &&
      entry.cwdPath === cwdPath &&
      !this.host.isClosed(entry.terminal)
    ) {
      return entry
    }

    if (entry) {
      this.sessions.delete(sessionKey)
      await this.host.dispose(entry.terminal)
    }
    const created = {
      terminal: await this.host.create(sessionKey, cwdPath),
      cwdPath,
    }
    this.sessions.set(sessionKey, created)
    return created
  }

  get(sessionKey: string): TerminalSessionEntry<Terminal> | undefined {
    return this.sessions.get(sessionKey)
  }

  entries(): Array<[string, TerminalSessionEntry<Terminal>]> {
    return [...this.sessions.entries()]
  }

  remove(sessionKey: string): TerminalSessionEntry<Terminal> | undefined {
    const entry = this.sessions.get(sessionKey)
    this.sessions.delete(sessionKey)
    return entry
  }
}

/** Normalize a Jupyter server path (always `/`, no `.` segments). */
export function normalizeServerPath(value: string): string | undefined {
  const normalized = value.replace(/\\/g, '/')
  if (normalized.startsWith('/') || /^[A-Za-z]:/.test(normalized)) {
    return undefined
  }
  const segments: string[] = []
  for (const segment of normalized.split('/')) {
    if (segment === '' || segment === '.') {
      continue
    }
    if (segment === '..') {
      return undefined
    }
    segments.push(segment)
  }
  return segments.join('/')
}

/**
 * `paths` are Course Workspace relative; an empty list or `.` means every
 * Cynapse terminal. Sessions under an excluded path survive.
 */
export function terminalCwdIsWithinResetPaths(
  paths: readonly string[],
  cwdPath: string,
  excludePaths: readonly string[] = [],
): boolean {
  const cwd = normalizeServerPath(cwdPath)
  if (cwd === undefined) {
    return false
  }
  const excluded = excludePaths.some((excludePath) => {
    const scope = normalizeServerPath(excludePath)
    return scope !== undefined && isWithin(scope, cwd)
  })
  if (excluded) {
    return false
  }
  if (paths.length === 0) {
    return true
  }
  return paths.some((resetPath) => {
    const scope = normalizeServerPath(resetPath)
    return scope !== undefined && isWithin(scope, cwd)
  })
}

export function terminalCwdsToDisposeUnderPaths<T extends TerminalSession>(
  sessions: readonly T[],
  paths: readonly string[],
  excludePaths: readonly string[] = [],
): T[] {
  return sessions.filter((session) =>
    terminalCwdIsWithinResetPaths(paths, session.cwdPath, excludePaths),
  )
}

function isWithin(scope: string, target: string): boolean {
  return scope === '' || target === scope || target.startsWith(`${scope}/`)
}
