import type { TerminalCommandAction } from './protocol'
import {
  MAX_TERMINAL_OUTPUT_BYTES,
  TerminalFacadeError,
  type TerminalExecutionResult,
  type TerminalFacade,
} from './terminal'
import {
  exitMarkerComplete,
  parseTerminalCommandOutput,
  wrapTerminalCommand,
  type TerminalShell,
} from './terminalCommand'
import {
  TerminalSessionManager,
  terminalCwdsToDisposeUnderPaths,
} from './terminalSessions'

export interface TerminalConnection {
  readonly name: string
  readonly closed: boolean
  send(data: string): void
  onOutput(listener: (data: string) => void): () => void
  onClose(listener: () => void): () => void
  dispose(): void
}

/** The Jupyter Server terminal operations the Adapter needs. */
export interface TerminalHost {
  create(cwd: string): Promise<TerminalConnection>
  delete(name: string): Promise<void>
  /** False for a missing file; a directory is a typed failure. */
  fileExists(path: string): Promise<boolean>
}

export interface JupyterServerTerminalFacadeOptions {
  shell?: TerminalShell
  interruptionGraceMs?: number
}

/**
 * A Terminal Session is one Jupyter Server terminal. The session manager keeps
 * the session key, the working directory, and the live connection together so a
 * check never depends on the current Kernel.
 */
export class JupyterServerTerminalFacade implements TerminalFacade {
  private readonly sessions: TerminalSessionManager<TerminalConnection>
  private readonly shell: TerminalShell
  private readonly interruptionGraceMs: number

  constructor(
    private readonly host: TerminalHost,
    options: JupyterServerTerminalFacadeOptions = {},
  ) {
    this.shell = options.shell ?? defaultTerminalShell()
    this.interruptionGraceMs = options.interruptionGraceMs ?? 2_000
    this.sessions = new TerminalSessionManager<TerminalConnection>({
      create: (_sessionKey, cwdPath) => host.create(cwdPath),
      isClosed: (connection) => connection.closed,
      dispose: async (connection) => {
        connection.dispose()
        await host.delete(connection.name).catch(() => undefined)
      },
    })
  }

  async ensure(
    sessionKey: string,
    cwd: string,
    newTerminal = false,
  ): Promise<void> {
    try {
      await this.sessions.ensure(sessionKey, cwd, newTerminal)
    } catch (error) {
      if (error instanceof TerminalFacadeError) {
        throw error
      }
      throw new TerminalFacadeError(
        'terminal_session_unavailable',
        `Could not create the Cynapse terminal: ${errorMessage(error)}`,
      )
    }
  }

  async completionMarkerExists(completionMarker: string): Promise<boolean> {
    return this.host.fileExists(completionMarker)
  }

  async disposeUnderPaths(
    paths: string[],
    excludePaths: string[] = [],
    sessionKeys: readonly string[] = [],
  ): Promise<void> {
    const sessions = this.sessions.entries().map(([sessionKey, entry]) => ({
      sessionKey,
      cwdPath: entry.cwdPath,
    }))
    // Session scope wins over path scope: an environment check closes exactly
    // the terminal it created, never the ones other Units left open.
    const matching =
      sessionKeys.length > 0
        ? sessions.filter(({ sessionKey }) => sessionKeys.includes(sessionKey))
        : terminalCwdsToDisposeUnderPaths(sessions, paths, excludePaths)
    const failures: string[] = []
    for (const { sessionKey } of matching) {
      const entry = this.sessions.remove(sessionKey)
      if (!entry) {
        continue
      }
      entry.terminal.dispose()
      try {
        await this.host.delete(entry.terminal.name)
      } catch (error) {
        failures.push(`${entry.terminal.name}: ${errorMessage(error)}`)
      }
    }
    if (failures.length > 0) {
      throw new TerminalFacadeError(
        'terminal_dispose_failed',
        `Could not close Cynapse terminals: ${failures.join(' | ')}`,
      )
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

    await this.ensure(action.session_key, action.cwd, action.new_terminal)
    const entry = this.sessions.get(action.session_key)
    if (!entry) {
      throw new TerminalFacadeError(
        'terminal_session_unavailable',
        'The Cynapse terminal disappeared before the command ran',
      )
    }
    if (entry.terminal.closed) {
      this.sessions.remove(action.session_key)
      throw new TerminalFacadeError(
        'terminal_session_unavailable',
        'The Cynapse terminal closed before the command ran',
      )
    }

    const outcome = await this.collect(
      entry.terminal,
      wrapTerminalCommand(this.shell, action.command),
      action.timeout_ms,
      signal,
    )
    if (outcome.kind !== 'complete') {
      await this.closeSession(action.session_key)
    }
    const parsed = parseTerminalCommandOutput(outcome.raw)
    return {
      exitCode: parsed.exitCode,
      output: parsed.output,
      outputTruncated: outcome.truncated,
      cancelled: outcome.kind === 'cancelled',
      timedOut: outcome.kind === 'timed_out',
    }
  }

  private async closeSession(sessionKey: string): Promise<void> {
    const entry = this.sessions.remove(sessionKey)
    if (!entry) {
      return
    }
    entry.terminal.dispose()
    await this.host.delete(entry.terminal.name).catch(() => undefined)
  }

  private collect(
    connection: TerminalConnection,
    wrapper: string,
    timeoutMs: number,
    signal?: AbortSignal,
  ): Promise<{
    kind: 'complete' | 'cancelled' | 'timed_out'
    raw: string
    truncated: boolean
  }> {
    return new Promise((resolve, reject) => {
      let raw = ''
      let truncated = false
      let phase: 'running' | 'grace' | 'done' = 'running'
      let interruption: 'cancelled' | 'timed_out' = 'cancelled'
      let graceTimer: NodeJS.Timeout | undefined
      let timeoutTimer: NodeJS.Timeout | undefined
      let detachOutput: () => void = () => {}
      let detachClose: () => void = () => {}
      const abortListener = (): void => requestInterruption('cancelled')

      const cleanup = (): void => {
        if (timeoutTimer) {
          clearTimeout(timeoutTimer)
        }
        if (graceTimer) {
          clearTimeout(graceTimer)
        }
        detachOutput()
        detachClose()
        signal?.removeEventListener('abort', abortListener)
      }

      const complete = (kind: 'complete' | 'cancelled' | 'timed_out'): void => {
        if (phase === 'done') {
          return
        }
        phase = 'done'
        cleanup()
        resolve({ kind, raw, truncated })
      }

      const requestInterruption = (kind: 'cancelled' | 'timed_out'): void => {
        if (phase !== 'running') {
          return
        }
        phase = 'grace'
        interruption = kind
        connection.send('\u0003')
        graceTimer = setTimeout(
          () => complete(interruption),
          this.interruptionGraceMs,
        )
        unrefTimer(graceTimer)
      }

      detachOutput = connection.onOutput((chunk) => {
        raw += chunk
        if (raw.length > MAX_TERMINAL_OUTPUT_BYTES * 2) {
          raw = raw.slice(-MAX_TERMINAL_OUTPUT_BYTES)
          truncated = true
        }
        if (!exitMarkerComplete(raw)) {
          return
        }
        complete(phase === 'running' ? 'complete' : interruption)
      })
      detachClose = connection.onClose(() => {
        if (phase === 'done') {
          return
        }
        phase = 'done'
        cleanup()
        reject(
          new TerminalFacadeError(
            'terminal_session_unavailable',
            'The Cynapse terminal closed while the command was running',
          ),
        )
      })

      if (signal?.aborted) {
        requestInterruption('cancelled')
        return
      }
      signal?.addEventListener('abort', abortListener, { once: true })
      timeoutTimer = setTimeout(
        () => requestInterruption('timed_out'),
        timeoutMs,
      )
      unrefTimer(timeoutTimer)
      connection.send(`${wrapper}\r`)
    })
  }
}

export function defaultTerminalShell(): TerminalShell {
  const navigatorValue = (
    globalThis as {
      navigator?: { platform?: string; userAgent?: string }
    }
  ).navigator
  if (
    navigatorValue &&
    /win/i.test(navigatorValue.platform ?? navigatorValue.userAgent ?? '')
  ) {
    return 'powershell'
  }
  const processValue = (globalThis as { process?: { platform?: string } })
    .process
  return processValue?.platform === 'win32' ? 'powershell' : 'posix'
}

export interface JupyterWebSocketLike {
  readonly readyState: number
  send(data: string): void
  close(): void
  addEventListener(type: string, listener: (event: unknown) => void): void
  removeEventListener(type: string, listener: (event: unknown) => void): void
}

export interface JupyterServerTerminalHostOptions {
  /** Jupyter Server base URL, for example `http://127.0.0.1:8888/`. */
  baseUrl: string
  fetch?: typeof fetch
  createWebSocket?: (url: string) => JupyterWebSocketLike
  openTimeoutMs?: number
}

/** Jupyter Server REST + WebSocket implementation of the terminal host. */
export function createJupyterServerTerminalHost(
  options: JupyterServerTerminalHostOptions,
): TerminalHost {
  const baseUrl = options.baseUrl.endsWith('/')
    ? options.baseUrl
    : `${options.baseUrl}/`
  const fetchImpl =
    options.fetch ??
    ((...args: Parameters<typeof fetch>) =>
      (globalThis.fetch as typeof fetch)(...args))
  const createWebSocket =
    options.createWebSocket ??
    ((url: string) =>
      new (
        globalThis as { WebSocket: new (url: string) => JupyterWebSocketLike }
      ).WebSocket(url))
  const openTimeoutMs = options.openTimeoutMs ?? 5_000

  return {
    async create(cwd) {
      const response = await fetchImpl(new URL('api/terminals', baseUrl), {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ cwd }),
      })
      if (!response.ok) {
        throw new TerminalFacadeError(
          'terminal_session_unavailable',
          `Jupyter Server could not create a terminal (${response.status})`,
        )
      }
      const model = (await response.json()) as { name?: unknown }
      if (typeof model.name !== 'string' || model.name.length === 0) {
        throw new TerminalFacadeError(
          'terminal_session_unavailable',
          'Jupyter Server returned an invalid terminal model',
        )
      }
      const socket = createWebSocket(terminalWebSocketUrl(baseUrl, model.name))
      await waitForOpen(socket, openTimeoutMs)
      return new JupyterServerTerminalConnection(model.name, socket)
    },

    async delete(name) {
      const response = await fetchImpl(
        new URL(`api/terminals/${encodeURIComponent(name)}`, baseUrl),
        { method: 'DELETE', credentials: 'same-origin' },
      )
      if (!response.ok && response.status !== 404) {
        throw new TerminalFacadeError(
          'terminal_dispose_failed',
          `Jupyter Server could not close terminal ${name} (${response.status})`,
        )
      }
    },

    async fileExists(path) {
      const response = await fetchImpl(
        new URL(`api/contents/${encodePath(path)}?content=0`, baseUrl),
        { credentials: 'same-origin' },
      )
      if (response.status === 404) {
        return false
      }
      if (!response.ok) {
        throw new TerminalFacadeError(
          'completion_marker_check_failed',
          `Jupyter Server could not read ${path} (${response.status})`,
        )
      }
      const model = (await response.json()) as { type?: unknown }
      if (model.type !== 'file') {
        throw new TerminalFacadeError(
          'completion_marker_not_file',
          `Completion marker ${path} is not a file`,
        )
      }
      return true
    },
  }
}

class JupyterServerTerminalConnection implements TerminalConnection {
  closed = false
  private readonly outputListeners = new Set<(data: string) => void>()
  private readonly closeListeners = new Set<() => void>()

  constructor(
    readonly name: string,
    private readonly socket: JupyterWebSocketLike,
  ) {
    socket.addEventListener('message', (event) => {
      const data = messageText(event)
      if (data === undefined) {
        return
      }
      let message: unknown
      try {
        message = JSON.parse(data)
      } catch {
        return
      }
      if (
        Array.isArray(message) &&
        message[0] === 'stdout' &&
        typeof message[1] === 'string'
      ) {
        for (const listener of [...this.outputListeners]) {
          listener(message[1])
        }
      }
    })
    socket.addEventListener('close', () => this.markClosed())
    socket.addEventListener('error', () => this.markClosed())
  }

  send(data: string): void {
    if (!this.closed) {
      this.socket.send(JSON.stringify(['stdin', data]))
    }
  }

  onOutput(listener: (data: string) => void): () => void {
    this.outputListeners.add(listener)
    return () => this.outputListeners.delete(listener)
  }

  onClose(listener: () => void): () => void {
    this.closeListeners.add(listener)
    return () => this.closeListeners.delete(listener)
  }

  dispose(): void {
    this.socket.close()
    this.markClosed()
  }

  private markClosed(): void {
    if (this.closed) {
      return
    }
    this.closed = true
    for (const listener of [...this.closeListeners]) {
      listener()
    }
  }
}

export function terminalWebSocketUrl(baseUrl: string, name: string): string {
  const url = new URL(
    `terminals/websocket/${encodeURIComponent(name)}`,
    baseUrl,
  )
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  return url.toString()
}

function waitForOpen(
  socket: JupyterWebSocketLike,
  timeoutMs: number,
): Promise<void> {
  if (socket.readyState === 1) {
    return Promise.resolve()
  }
  return new Promise((resolve, reject) => {
    const cleanup = (): void => {
      clearTimeout(timer)
      socket.removeEventListener('open', open)
      socket.removeEventListener('error', failed)
      socket.removeEventListener('close', failed)
    }
    const open = (): void => {
      cleanup()
      resolve()
    }
    const failed = (): void => {
      cleanup()
      reject(
        new TerminalFacadeError(
          'terminal_session_unavailable',
          'The Cynapse terminal connection closed before it was ready',
        ),
      )
    }
    const timer = setTimeout(() => {
      cleanup()
      reject(
        new TerminalFacadeError(
          'terminal_session_unavailable',
          `The Cynapse terminal did not connect within ${timeoutMs} ms`,
        ),
      )
    }, timeoutMs)
    unrefTimer(timer)
    socket.addEventListener('open', open)
    socket.addEventListener('error', failed)
    socket.addEventListener('close', failed)
  })
}

function messageText(event: unknown): string | undefined {
  const data = (event as { data?: unknown }).data
  if (typeof data === 'string') {
    return data
  }
  if (
    data !== null &&
    typeof data === 'object' &&
    typeof (data as { toString?: unknown }).toString === 'function'
  ) {
    return String(data)
  }
  return undefined
}

/** Keep an abandoned test timer from holding the Node process open. */
function unrefTimer(timer: ReturnType<typeof setTimeout>): void {
  const unref = (timer as { unref?: () => void }).unref
  unref?.call(timer)
}

function encodePath(value: string): string {
  return value
    .replace(/\\/g, '/')
    .replace(/^\/+/, '')
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/')
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
