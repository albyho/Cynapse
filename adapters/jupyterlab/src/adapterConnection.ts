import {
  AdapterConnection,
  parseWorkspaceResetPayload,
  type ActionContext,
  type ActionHandler,
  type AdapterTransport,
  type CheckHandler as SharedCheckHandler,
} from '@cynapse/adapter-kit'

import type { Envelope } from './protocol'
import type {
  WorkspaceResetHandler,
  WorkspaceResetRequest,
} from './workspaceReset'

/** Fixed loopback control port shared with the Client (ADR 0066). */
export const DEFAULT_ADAPTER_PORT = 37654
export const DEFAULT_RECONNECT_DELAY_MS = 1000
/** Client close code: a newer JupyterLab page now owns the control channel. */
export const INACTIVE_ADAPTER_CLOSE_CODE = 4001

const REJECTION_CODES = [
  'version_mismatch',
  'invalid_origin',
  'invalid_message',
] as const

export type { ActionContext }
export type CheckHandler = SharedCheckHandler

export type JupyterLabAdapterStatus =
  | { kind: 'waiting'; message: string }
  | { kind: 'connected'; adapter: string }
  | { kind: 'inactive'; message: string }
  | { kind: 'error'; message: string }

export interface JupyterLabAdapterConfig {
  /** Overrides the loopback control port; tests use an ephemeral port. */
  port?: number
  /** Overrides the delay before the next connection attempt; tests use ~10 ms. */
  reconnectDelayMs?: number
}

class BrowserWebSocketTransport implements AdapterTransport {
  private readonly socket: WebSocket
  private openHandler: (() => void) | undefined
  private messageHandler: ((data: string) => void) | undefined
  private closeHandler:
    ((event: { code: number; reason: string }) => void) | undefined
  private errorHandler: (() => void) | undefined

  constructor(url: string) {
    this.socket = new WebSocket(url)
    this.socket.addEventListener('open', () => this.openHandler?.())
    this.socket.addEventListener('message', (event) => {
      this.messageHandler?.(String(event.data))
    })
    this.socket.addEventListener('close', (event) => {
      this.closeHandler?.({ code: event.code, reason: event.reason })
    })
    this.socket.addEventListener('error', () => this.errorHandler?.())
  }

  send(data: string): void {
    this.socket.send(data)
  }

  close(): void {
    this.socket.close()
  }

  isOpen(): boolean {
    return this.socket.readyState === WebSocket.OPEN
  }

  onOpen(handler: () => void): void {
    this.openHandler = handler
  }

  onMessage(handler: (data: string) => void): void {
    this.messageHandler = handler
  }

  onClose(handler: (event: { code: number; reason: string }) => void): void {
    this.closeHandler = handler
  }

  onError(handler: () => void): void {
    this.errorHandler = handler
  }
}

/**
 * The extension-side half of the Cynapse Protocol connection. Platform
 * behavior stays here; queueing, ACK dedupe, cancel, and reconnect live in
 * `@cynapse/adapter-kit`.
 */
export class JupyterLabAdapterConnection {
  private readonly connection: AdapterConnection

  constructor(
    config: JupyterLabAdapterConfig,
    onStatus: (status: JupyterLabAdapterStatus) => void,
    onAction: ActionHandler = async () => ({
      status: 'error',
      code: 'unhandled_action',
      message: 'No JupyterLab action handler is installed',
    }),
    onCheck: CheckHandler = async () => ({
      status: 'error',
      code: 'unhandled_check',
      message: 'No JupyterLab check handler is installed',
    }),
    onWorkspaceReset: WorkspaceResetHandler = async () => ({
      status: 'error',
      code: 'unhandled_workspace_reset',
      message: 'No workspace reset handler is installed',
    }),
  ) {
    this.connection = new AdapterConnection({
      createTransport: () =>
        new BrowserWebSocketTransport(
          `ws://127.0.0.1:${config.port ?? DEFAULT_ADAPTER_PORT}`,
        ),
      createId: () => crypto.randomUUID(),
      helloToken: '',
      adapterName: 'jupyterlab',
      rejectionCodes: REJECTION_CODES,
      reconnectDelayMs: config.reconnectDelayMs ?? DEFAULT_RECONNECT_DELAY_MS,
      inactiveCloseCode: INACTIVE_ADAPTER_CLOSE_CODE,
      messages: {
        connecting: 'Connecting to Cynapse Client…',
        waiting: 'Waiting for Cynapse Client…',
        handshakeFailed: 'Handshake failed',
        workspaceResetFailed: 'Workspace reset failed',
      },
      onStatus: (status) => {
        onStatus(status)
      },
      onAction,
      onCheck,
      onWorkspaceReset: async (envelope: Envelope, context) => {
        const payload = parseWorkspaceResetPayload(envelope.payload)
        if (!payload) {
          return {
            status: 'error',
            code: 'invalid_workspace_reset',
            message: 'Invalid workspace reset payload',
          }
        }
        const request: WorkspaceResetRequest = {
          paths: payload.paths,
          excludePaths: payload.exclude_paths ?? [],
          closeTabs: payload.close_tabs ?? true,
          ...(payload.session_keys
            ? { sessionKeys: payload.session_keys }
            : {}),
        }
        return onWorkspaceReset(request, context)
      },
      onHandlerFailure: (envelope, payload) => {
        const actionType =
          typeof envelope.payload.type === 'string'
            ? envelope.payload.type
            : envelope.type
        console.error(
          '[cynapse] action.failed',
          actionType,
          payload.code ?? '',
          payload.message ?? '',
        )
      },
    })
  }

  connect(): void {
    this.connection.connect()
  }

  dispose(): void {
    this.connection.dispose()
  }
}
