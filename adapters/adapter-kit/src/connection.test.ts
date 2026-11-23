import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  AdapterConnection,
  parseWorkspaceResetPayload,
  type AdapterStatus,
  type AdapterTransport,
} from './connection'
import type { AckPayload, Envelope } from './protocol'
import { PROTOCOL_VERSION } from './protocol'

class FakeTransport implements AdapterTransport {
  readonly sent: Envelope[] = []
  private openHandler: (() => void) | undefined
  private messageHandler: ((data: string) => void) | undefined
  private closeHandler:
    ((event: { code: number; reason: string }) => void) | undefined

  send(data: string): void {
    this.sent.push(JSON.parse(data) as Envelope)
  }

  close(): void {}

  isOpen(): boolean {
    return true
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

  onError(): void {}

  open(): void {
    this.openHandler?.()
  }

  message(envelope: Envelope): void {
    this.messageHandler?.(JSON.stringify(envelope))
  }

  closeWith(code = 1000, reason = ''): void {
    this.closeHandler?.({ code, reason })
  }
}

function createConnection(
  transport: FakeTransport,
  statuses: AdapterStatus[],
  onAction: (envelope: Envelope) => Promise<AckPayload>,
) {
  return new AdapterConnection({
    createTransport: () => transport,
    createId: () => 'hello-id',
    helloToken: 'token',
    adapterName: 'test-adapter',
    rejectionCodes: ['invalid_message'],
    inactiveCloseCode: 4001,
    messages: {
      connecting: 'connecting',
      waiting: 'waiting',
      handshakeFailed: 'handshake failed',
      workspaceResetFailed: 'workspace reset failed',
    },
    onStatus: (status) => statuses.push(status),
    onAction: (envelope) => onAction(envelope),
    onCheck: async () => ({ status: 'done' }),
    onWorkspaceReset: async () => ({ status: 'done' }),
  })
}

test('connects, sends hello, and deduplicates a repeated action ACK', async () => {
  const transport = new FakeTransport()
  const statuses: AdapterStatus[] = []
  let executions = 0
  const connection = createConnection(transport, statuses, async () => {
    executions += 1
    return { status: 'done' }
  })

  connection.connect()
  transport.open()
  assert.deepEqual(transport.sent[0], {
    v: PROTOCOL_VERSION,
    id: 'hello-id',
    type: 'hello',
    payload: { token: 'token', role: 'adapter' },
  })

  transport.message({
    v: PROTOCOL_VERSION,
    id: 'hello',
    type: 'hello_ack',
    payload: { adapter: 'test-adapter' },
  })
  assert.deepEqual(statuses.at(-1), {
    kind: 'connected',
    adapter: 'test-adapter',
  })

  const action: Envelope = {
    v: PROTOCOL_VERSION,
    id: 'action-1',
    type: 'action',
    payload: {},
  }
  transport.message(action)
  await new Promise((resolve) => setImmediate(resolve))
  transport.message(action)
  await new Promise((resolve) => setImmediate(resolve))

  assert.equal(executions, 1)
  assert.equal(
    transport.sent.filter((envelope) => envelope.type === 'ack').length,
    2,
  )
  connection.dispose()
})

test('uses the configured inactive close code without reconnecting', () => {
  const transport = new FakeTransport()
  const statuses: AdapterStatus[] = []
  const connection = createConnection(transport, statuses, async () => ({
    status: 'done',
  }))

  connection.connect()
  transport.open()
  transport.closeWith(4001, 'inactive')

  assert.deepEqual(statuses.at(-1), {
    kind: 'inactive',
    message: 'inactive',
  })
  connection.dispose()
})
test('parses a session-scoped cleanup and rejects invalid session keys', () => {
  assert.deepEqual(
    parseWorkspaceResetPayload({
      paths: [],
      session_keys: ['environment'],
      close_tabs: false,
    }),
    { paths: [], session_keys: ['environment'], close_tabs: false },
  )
  assert.equal(
    parseWorkspaceResetPayload({ paths: [], session_keys: [1] }),
    undefined,
  )
  assert.equal(
    parseWorkspaceResetPayload({ paths: [], session_keys: 'environment' }),
    undefined,
  )
})
