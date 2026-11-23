import {
  PROTOCOL_VERSION,
  type AckPayload,
  type CancelPayload,
  type Envelope,
  type HelloAckPayload,
  type WorkspaceResetPayload,
} from './protocol'

const DEDUPE_WINDOW_MS = 60_000
const DEDUPE_LIMIT = 100

export interface AdapterTransport {
  send(data: string): void
  close(): void
  isOpen(): boolean
  onOpen(handler: () => void): void
  onMessage(handler: (data: string) => void): void
  onClose(handler: (event: { code: number; reason: string }) => void): void
  onError(handler: () => void): void
}

export type AdapterStatus =
  | { kind: 'waiting'; message: string }
  | { kind: 'connected'; adapter: string }
  | { kind: 'inactive'; message: string }
  | { kind: 'error'; message: string }

export interface ActionContext {
  signal: AbortSignal
}

export type ActionHandler = (
  envelope: Envelope,
  context: ActionContext,
) => Promise<AckPayload>

export type CheckHandler = ActionHandler

export type WorkspaceResetHandler = (
  envelope: Envelope,
  context: ActionContext,
) => Promise<AckPayload>

export interface AdapterConnectionMessages {
  connecting: string
  waiting: string
  handshakeFailed: string
  workspaceResetFailed: string
}

export interface AdapterConnectionOptions {
  createTransport: () => AdapterTransport
  createId: () => string
  helloToken: string
  adapterName: string
  rejectionCodes: readonly string[]
  reconnectDelayMs?: number
  inactiveCloseCode?: number
  messages: AdapterConnectionMessages
  onStatus: (status: AdapterStatus) => void
  onAction: ActionHandler
  onCheck: ActionHandler
  onWorkspaceReset: WorkspaceResetHandler
  onEnqueue?: () => void
  onHandlerFailure?: (envelope: Envelope, payload: AckPayload) => void
}

/**
 * Transport-neutral Protocol connection. Platform adapters provide the
 * WebSocket implementation and parse their own Workspace Reset payload.
 */
export class AdapterConnection {
  private transport: AdapterTransport | undefined
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined
  private disposed = false
  private rejected = false
  private actionQueue: Promise<void> = Promise.resolve()
  private activeAction: { id: string; controller: AbortController } | undefined
  private readonly cancelledRequests = new Set<string>()
  private readonly pendingRequests = new Map<string, Promise<AckPayload>>()
  private readonly recentAcks = new Map<
    string,
    { payload: AckPayload; completedAt: number }
  >()

  constructor(private readonly options: AdapterConnectionOptions) {}

  connect(): void {
    if (this.disposed || this.transport) {
      return
    }

    this.options.onStatus({
      kind: 'waiting',
      message: this.options.messages.connecting,
    })

    const transport = this.options.createTransport()
    this.transport = transport
    transport.onOpen(() => {
      const hello: Envelope = {
        v: PROTOCOL_VERSION,
        id: this.options.createId(),
        type: 'hello',
        payload: {
          token: this.options.helloToken,
          role: 'adapter',
        },
      }
      transport.send(JSON.stringify(hello))
    })
    transport.onMessage((data) => this.receiveMessage(data))
    transport.onClose((event) => {
      this.transport = undefined
      this.activeAction?.controller.abort()
      if (this.disposed) {
        return
      }
      if (
        this.options.inactiveCloseCode !== undefined &&
        event.code === this.options.inactiveCloseCode
      ) {
        this.options.onStatus({
          kind: 'inactive',
          message: event.reason || this.options.messages.waiting,
        })
        return
      }
      this.options.onStatus({
        kind: 'waiting',
        message: this.options.messages.waiting,
      })
      if (!this.rejected) {
        this.scheduleReconnect()
      }
    })
    transport.onError(() => {
      // The close handler owns status updates and reconnect scheduling.
    })
  }

  dispose(): void {
    this.disposed = true
    this.activeAction?.controller.abort()
    if (this.reconnectTimer !== undefined) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = undefined
    }
    this.transport?.close()
    this.transport = undefined
  }

  private scheduleReconnect(): void {
    if (this.disposed || this.reconnectTimer !== undefined) {
      return
    }
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = undefined
      this.connect()
    }, this.options.reconnectDelayMs ?? 1000)
  }

  private receiveMessage(raw: string): void {
    const envelope = parseEnvelope(raw)
    if (!envelope) {
      return
    }

    switch (envelope.type) {
      case 'hello_ack': {
        const payload = envelope.payload as Partial<HelloAckPayload>
        this.options.onStatus({
          kind: 'connected',
          adapter: payload.adapter ?? this.options.adapterName,
        })
        return
      }
      case 'action':
        this.receiveAction(envelope)
        return
      case 'check':
        this.receiveCheck(envelope)
        return
      case 'workspace_reset':
        this.receiveWorkspaceReset(envelope)
        return
      case 'cancel': {
        const payload = envelope.payload as Partial<CancelPayload>
        if (typeof payload.target_id !== 'string') {
          return
        }
        if (this.activeAction?.id === payload.target_id) {
          this.activeAction.controller.abort()
        } else if (this.pendingRequests.has(payload.target_id)) {
          this.cancelledRequests.add(payload.target_id)
        }
        return
      }
      case 'ack': {
        const payload = envelope.payload as Partial<AckPayload>
        if (
          payload.status === 'error' &&
          this.options.rejectionCodes.includes(payload.code ?? '')
        ) {
          this.rejected = true
          this.options.onStatus({
            kind: 'error',
            message:
              payload.message ??
              payload.code ??
              this.options.messages.handshakeFailed,
          })
        }
        return
      }
      default:
        return
    }
  }

  private receiveAction(envelope: Envelope): void {
    this.receiveOneOff(envelope, this.options.onAction)
  }

  private receiveCheck(envelope: Envelope): void {
    this.receiveOneOff(envelope, this.options.onCheck)
  }

  private receiveOneOff(envelope: Envelope, handler: ActionHandler): void {
    if (this.disposed) {
      return
    }
    const cached = this.getCachedAck(envelope.id)
    if (cached) {
      this.sendAck(envelope.id, cached)
      return
    }
    const pending = this.pendingRequests.get(envelope.id)
    if (pending) {
      void pending
        .then((payload) => this.sendAck(envelope.id, payload))
        .catch(() => undefined)
      return
    }
    const task = this.enqueue(async () => {
      const payload = await this.handleOneOff(envelope, handler)
      this.cacheAck(envelope.id, payload)
      this.sendAck(envelope.id, payload)
      return payload
    })
    this.pendingRequests.set(envelope.id, task)
    void task.then(
      () => this.pendingRequests.delete(envelope.id),
      () => this.pendingRequests.delete(envelope.id),
    )
  }

  private async handleOneOff(
    envelope: Envelope,
    handler: ActionHandler,
  ): Promise<AckPayload> {
    if (this.disposed) {
      return { status: 'cancelled' }
    }
    if (this.cancelledRequests.delete(envelope.id)) {
      return { status: 'cancelled' }
    }

    const controller = new AbortController()
    this.activeAction = { id: envelope.id, controller }
    let payload: AckPayload
    try {
      payload = await handler(envelope, { signal: controller.signal })
    } catch (error) {
      payload = {
        status: 'error',
        code: 'apply_failed',
        message: error instanceof Error ? error.message : String(error),
      }
    } finally {
      this.cancelledRequests.delete(envelope.id)
      if (this.activeAction?.id === envelope.id) {
        this.activeAction = undefined
      }
    }

    if (payload.status === 'error') {
      this.options.onHandlerFailure?.(envelope, payload)
    }
    return payload
  }

  private receiveWorkspaceReset(envelope: Envelope): void {
    if (this.disposed) {
      return
    }
    const cached = this.getCachedAck(envelope.id)
    if (cached) {
      this.sendAck(envelope.id, cached)
      return
    }
    const pending = this.pendingRequests.get(envelope.id)
    if (pending) {
      void pending
        .then((payload) => this.sendAck(envelope.id, payload))
        .catch(() => undefined)
      return
    }
    const task = this.enqueue(async () => {
      const payload = await this.handleWorkspaceReset(envelope)
      this.cacheAck(envelope.id, payload)
      this.sendAck(envelope.id, payload)
      return payload
    })
    this.pendingRequests.set(envelope.id, task)
    void task.then(
      () => this.pendingRequests.delete(envelope.id),
      () => this.pendingRequests.delete(envelope.id),
    )
  }

  private async handleWorkspaceReset(envelope: Envelope): Promise<AckPayload> {
    if (this.disposed) {
      return { status: 'cancelled' }
    }
    if (this.cancelledRequests.delete(envelope.id)) {
      return { status: 'cancelled' }
    }

    const controller = new AbortController()
    this.activeAction = { id: envelope.id, controller }
    let payload: AckPayload
    try {
      payload = await this.options.onWorkspaceReset(envelope, {
        signal: controller.signal,
      })
    } catch (error) {
      payload = {
        status: 'error',
        code: 'workspace_reset_failed',
        message:
          error instanceof Error
            ? error.message
            : this.options.messages.workspaceResetFailed,
      }
    } finally {
      this.cancelledRequests.delete(envelope.id)
      if (this.activeAction?.id === envelope.id) {
        this.activeAction = undefined
      }
    }

    return payload
  }

  private enqueue<T>(task: () => Promise<T>): Promise<T> {
    this.options.onEnqueue?.()
    const run = this.actionQueue.then(task, task)
    this.actionQueue = run.then(
      () => undefined,
      () => undefined,
    )
    return run
  }

  private sendAck(actionId: string, payload: AckPayload): void {
    if (!this.transport?.isOpen()) {
      return
    }
    const ack: Envelope = {
      v: PROTOCOL_VERSION,
      id: actionId,
      type: 'ack',
      payload: payload as unknown as Record<string, unknown>,
    }
    this.transport.send(JSON.stringify(ack))
  }

  private getCachedAck(actionId: string): AckPayload | undefined {
    const cached = this.recentAcks.get(actionId)
    if (!cached) {
      return undefined
    }
    if (Date.now() - cached.completedAt > DEDUPE_WINDOW_MS) {
      this.recentAcks.delete(actionId)
      return undefined
    }
    return cached.payload
  }

  private cacheAck(actionId: string, payload: AckPayload): void {
    this.recentAcks.set(actionId, {
      payload,
      completedAt: Date.now(),
    })
    while (this.recentAcks.size > DEDUPE_LIMIT) {
      const oldest = this.recentAcks.keys().next().value
      if (typeof oldest !== 'string') {
        break
      }
      this.recentAcks.delete(oldest)
    }
  }
}

export function parseEnvelope(raw: string): Envelope | undefined {
  try {
    const parsed = JSON.parse(raw) as Partial<Envelope>
    if (
      parsed.v !== PROTOCOL_VERSION ||
      typeof parsed.id !== 'string' ||
      typeof parsed.type !== 'string' ||
      (parsed.unit_path !== undefined &&
        typeof parsed.unit_path !== 'string') ||
      typeof parsed.payload !== 'object' ||
      parsed.payload === null
    ) {
      return undefined
    }
    return parsed as Envelope
  } catch {
    return undefined
  }
}

export function parseWorkspaceResetPayload(
  payload: Record<string, unknown>,
): WorkspaceResetPayload | undefined {
  const request = payload as Partial<WorkspaceResetPayload>
  const paths = request.paths
  const excludePaths = request.exclude_paths ?? []
  const sessionKeys = request.session_keys ?? []
  if (
    !isWorkspacePathList(paths) ||
    !isWorkspacePathList(excludePaths) ||
    !isWorkspacePathList(sessionKeys) ||
    (request.close_tabs !== undefined &&
      typeof request.close_tabs !== 'boolean')
  ) {
    return undefined
  }
  return {
    paths,
    ...(excludePaths.length > 0 ? { exclude_paths: excludePaths } : {}),
    ...(sessionKeys.length > 0 ? { session_keys: sessionKeys } : {}),
    ...(request.close_tabs !== undefined
      ? { close_tabs: request.close_tabs }
      : {}),
  }
}

function isWorkspacePathList(value: unknown): value is string[] {
  return (
    Array.isArray(value) &&
    value.every((path) => typeof path === 'string' && path.length > 0)
  )
}
