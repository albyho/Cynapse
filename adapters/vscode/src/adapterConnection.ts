import { randomUUID } from 'node:crypto'

import WebSocket from 'ws'

import {
  AdapterConnection,
  parseWorkspaceResetPayload,
  type AckPayload,
  type ActionContext,
  type ActionHandler,
  type AdapterTransport,
  type Envelope,
  type WorkspaceResetPayload,
  type CheckHandler as SharedCheckHandler,
} from '@cynapse/adapter-kit'

import type { Discovery } from './discovery'
import { t } from './i18n'

export type VSCodeAdapterStatus =
  | { kind: 'waiting'; message: string }
  | { kind: 'connected'; adapter: string }
  | { kind: 'error'; message: string }

export type { ActionContext, ActionHandler }
export type CheckHandler = SharedCheckHandler

export type WorkspaceResetHandler = (
  payload: WorkspaceResetPayload,
) => Promise<AckPayload>

class WebSocketTransport implements AdapterTransport {
  private readonly socket: WebSocket
  private openHandler: (() => void) | undefined
  private messageHandler: ((data: string) => void) | undefined
  private closeHandler:
    ((event: { code: number; reason: string }) => void) | undefined
  private errorHandler: (() => void) | undefined

  constructor(port: number) {
    this.socket = new WebSocket(`ws://127.0.0.1:${port}`)
    this.socket.on('open', () => this.openHandler?.())
    this.socket.on('message', (data) => {
      this.messageHandler?.(data.toString())
    })
    this.socket.on('close', (code, reason) => {
      this.closeHandler?.({ code, reason: reason.toString() })
    })
    this.socket.on('error', () => this.errorHandler?.())
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

export class VSCodeAdapterConnection {
  private readonly connection: AdapterConnection

  constructor(
    discovery: Discovery,
    onStatus: (status: VSCodeAdapterStatus) => void,
    onAction: ActionHandler = async () => ({
      status: 'error',
      code: 'unhandled_action',
      message: t('No action handler is installed'),
    }),
    onCheck: CheckHandler = async () => ({
      status: 'error',
      code: 'unhandled_check',
      message: t('No check handler is installed'),
    }),
    onWorkspaceReset: WorkspaceResetHandler = async () => ({
      status: 'error',
      code: 'unhandled_workspace_reset',
      message: t('No workspace reset handler is installed'),
    }),
    onEnqueue: () => void = () => {},
  ) {
    this.connection = new AdapterConnection({
      createTransport: () => new WebSocketTransport(discovery.port),
      createId: randomUUID,
      helloToken: discovery.token,
      adapterName: 'vscode-precise',
      rejectionCodes: ['version_mismatch', 'token_mismatch', 'invalid_message'],
      messages: {
        connecting: t('Connecting to Client...'),
        waiting: t('Waiting for Client'),
        handshakeFailed: t('Handshake failed'),
        workspaceResetFailed: t('Workspace reset failed'),
      },
      onStatus: (status) => {
        if (status.kind === 'connected') {
          onStatus({ kind: 'connected', adapter: status.adapter })
        } else if (status.kind === 'waiting' || status.kind === 'error') {
          onStatus(status)
        }
      },
      onAction,
      onCheck,
      onWorkspaceReset: async (envelope: Envelope) => {
        const payload = parseWorkspaceResetPayload(envelope.payload)
        if (!payload) {
          return {
            status: 'error',
            code: 'invalid_workspace_reset',
            message: t('Invalid workspace reset payload'),
          }
        }
        return onWorkspaceReset(payload)
      },
      onEnqueue,
    })
  }

  connect(): void {
    this.connection.connect()
  }

  dispose(): void {
    this.connection.dispose()
  }
}
