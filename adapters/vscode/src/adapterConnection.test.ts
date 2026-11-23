import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { once } from 'node:events'
import type { AddressInfo } from 'node:net'
import { test } from 'node:test'

import WebSocket, { WebSocketServer } from 'ws'

import {
  VSCodeAdapterConnection,
  type ActionHandler,
  type WorkspaceResetHandler,
} from './adapterConnection'
import {
  PROTOCOL_VERSION,
  type AckPayload,
  type Envelope,
} from '@cynapse/adapter-kit'

interface Harness {
  sendAction: (id: string, code: string) => void
  sendActionPayload: (id: string, payload: Record<string, unknown>) => void
  sendCancel: (targetId: string) => void
  sendWorkspaceReset: (
    id: string,
    paths: string[],
    closeTabs?: boolean,
    excludePaths?: string[],
  ) => void
  waitForAck: (id: string, occurrence?: number) => Promise<AckPayload>
  disconnectAdapter: () => Promise<void>
  close: () => Promise<void>
}

async function startHarness(
  handler: ActionHandler,
  workspaceResetHandler?: WorkspaceResetHandler,
  onEnqueue?: () => void,
): Promise<Harness> {
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  await once(server, 'listening')
  const address = server.address() as AddressInfo
  const messages: Envelope[] = []
  const statuses: string[] = []

  const socketPromise = new Promise<WebSocket>((resolve) => {
    server.once('connection', (socket) => {
      socket.on('message', (data) => {
        messages.push(JSON.parse(data.toString()) as Envelope)
      })
      resolve(socket)
    })
  })

  const adapter = new VSCodeAdapterConnection(
    {
      port: address.port,
      token: 'test-token',
      pid: process.pid,
      started_at: Date.now(),
    },
    (status) => {
      statuses.push(status.kind)
    },
    handler,
    undefined,
    workspaceResetHandler,
    onEnqueue,
  )
  adapter.connect()
  const socket = await socketPromise

  await waitFor(() => messages.some((message) => message.type === 'hello'))
  socket.send(
    JSON.stringify({
      v: PROTOCOL_VERSION,
      id: randomUUID(),
      type: 'hello_ack',
      payload: { adapter: 'vscode-precise' },
    } satisfies Envelope),
  )
  await waitFor(() => statuses.includes('connected'))

  return {
    sendAction: (id, code) => {
      const sendActionPayload = (
        actionId: string,
        actionPayload: Record<string, unknown>,
      ): void => {
        socket.send(
          JSON.stringify({
            v: PROTOCOL_VERSION,
            id: actionId,
            type: 'action',
            payload: actionPayload,
          } satisfies Envelope),
        )
      }
      sendActionPayload(id, {
        type: 'InsertText',
        path: 'src/main.rs',
        target: { line: 1 },
        speed_ms: 30,
        text: code,
      })
    },
    sendActionPayload: (id, payload) => {
      socket.send(
        JSON.stringify({
          v: PROTOCOL_VERSION,
          id,
          type: 'action',
          payload,
        } satisfies Envelope),
      )
    },
    sendCancel: (targetId) => {
      socket.send(
        JSON.stringify({
          v: PROTOCOL_VERSION,
          id: randomUUID(),
          type: 'cancel',
          payload: { target_id: targetId },
        } satisfies Envelope),
      )
    },
    sendWorkspaceReset: (id, paths, closeTabs, excludePaths) => {
      socket.send(
        JSON.stringify({
          v: PROTOCOL_VERSION,
          id,
          type: 'workspace_reset',
          payload: {
            paths,
            ...(closeTabs === undefined ? {} : { close_tabs: closeTabs }),
            ...(excludePaths === undefined
              ? {}
              : { exclude_paths: excludePaths }),
          },
        } satisfies Envelope),
      )
    },
    waitForAck: async (id, occurrence = 1) => {
      const matchingAcks = () =>
        messages.filter(
          (message) => message.type === 'ack' && message.id === id,
        )
      await waitFor(() => matchingAcks().length >= occurrence)
      const ack = matchingAcks()[occurrence - 1]
      assert.ok(ack)
      return ack.payload as unknown as AckPayload
    },
    disconnectAdapter: async () => {
      socket.close()
      await once(socket, 'close')
    },
    close: async () => {
      adapter.dispose()
      socket.close()
      server.close()
      await once(server, 'close')
    },
  }
}

test('re-sends an ack for a duplicate action without executing it twice', async () => {
  let executions = 0
  const harness = await startHarness(async () => {
    executions += 1
    return { status: 'done' }
  })
  const actionId = randomUUID()

  harness.sendAction(actionId, 'fn main() {}')
  const firstAck = await harness.waitForAck(actionId)
  harness.sendAction(actionId, 'fn main() {}')
  const secondAck = await harness.waitForAck(actionId, 2)

  assert.equal(executions, 1)
  assert.equal(firstAck.status, 'done')
  assert.equal(secondAck.status, 'done')
  await harness.close()
})

test('aborts the active action when a matching cancel arrives', async () => {
  let started!: () => void
  const actionStarted = new Promise<void>((resolve) => {
    started = resolve
  })
  const harness = await startHarness(
    async (_action, { signal }) =>
      new Promise<AckPayload>((resolve) => {
        started()
        signal.addEventListener(
          'abort',
          () => resolve({ status: 'cancelled' }),
          { once: true },
        )
      }),
  )
  const actionId = randomUUID()

  harness.sendAction(actionId, 'long action')
  await actionStarted
  harness.sendCancel(actionId)
  const ack = await harness.waitForAck(actionId)

  assert.equal(ack.status, 'cancelled')
  await harness.close()
})

test('reports a queued action as cancelled before it starts', async () => {
  let executions = 0
  let enqueued = 0
  let started!: () => void
  const actionStarted = new Promise<void>((resolve) => {
    started = resolve
  })
  const harness = await startHarness(
    async (_action, { signal }) => {
      executions += 1
      started()
      return new Promise<AckPayload>((resolve) => {
        signal.addEventListener(
          'abort',
          () => resolve({ status: 'cancelled' }),
          { once: true },
        )
      })
    },
    undefined,
    () => {
      enqueued += 1
    },
  )
  const first = randomUUID()
  const second = randomUUID()

  harness.sendAction(first, 'first action')
  await actionStarted
  harness.sendAction(second, 'queued action')
  await waitFor(() => enqueued >= 2)
  harness.sendCancel(second)
  harness.sendCancel(first)

  const firstAck = await harness.waitForAck(first)
  const secondAck = await harness.waitForAck(second)

  assert.equal(firstAck.status, 'cancelled')
  assert.equal(secondAck.status, 'cancelled')
  assert.equal(executions, 1)
  await harness.close()
})

test('aborts the active action when the adapter connection closes', async () => {
  let actionAborted!: () => void
  const aborted = new Promise<void>((resolve) => {
    actionAborted = resolve
  })
  const harness = await startHarness(
    async (_action, { signal }) =>
      new Promise<AckPayload>((resolve) => {
        signal.addEventListener(
          'abort',
          () => {
            actionAborted()
            resolve({ status: 'cancelled' })
          },
          { once: true },
        )
      }),
  )

  harness.sendAction(randomUUID(), 'long action')
  await new Promise((resolve) => setTimeout(resolve, 10))
  await harness.disconnectAdapter()
  await Promise.race([
    aborted,
    new Promise((_, reject) =>
      setTimeout(() => reject(new Error('active action was not aborted')), 500),
    ),
  ])
  await harness.close()
})

test('drops queued actions and workspace resets after dispose', async () => {
  let actionExecutions = 0
  let resets = 0
  let enqueued = 0
  let started!: () => void
  const actionStarted = new Promise<void>((resolve) => {
    started = resolve
  })
  const harness = await startHarness(
    async (_action, { signal }) => {
      actionExecutions += 1
      if (actionExecutions > 1) {
        return { status: 'done' }
      }
      started()
      return new Promise<AckPayload>((resolve) => {
        signal.addEventListener(
          'abort',
          () => resolve({ status: 'cancelled' }),
          { once: true },
        )
      })
    },
    async () => {
      resets += 1
      return { status: 'done' }
    },
    () => {
      enqueued += 1
    },
  )

  harness.sendAction(randomUUID(), 'long action')
  await actionStarted
  harness.sendAction(randomUUID(), 'queued action')
  harness.sendWorkspaceReset(randomUUID(), ['src'])
  // Wait until the active action, queued action, and reset are all enqueued.
  await waitFor(() => enqueued >= 3)

  await harness.close()
  await new Promise((resolve) => setTimeout(resolve, 50))

  assert.equal(actionExecutions, 1)
  assert.equal(resets, 0)
})

test('deduplicates terminal commands by action id', async () => {
  let executions = 0
  const harness = await startHarness(async () => {
    executions += 1
    return {
      status: 'done',
      exit_code: 0,
      output: 'Hello, world!\n',
    }
  })
  const actionId = randomUUID()
  const payload = {
    type: 'TerminalCommand',
    session_key: 'main',
    cwd: 'hello_world',
    command: '.\\main.exe',
    accepted_exit_codes: [0],
    expected_output: ['Hello, world!'],
    timeout_ms: 300000,
  }

  harness.sendActionPayload(actionId, payload)
  await harness.waitForAck(actionId)
  harness.sendActionPayload(actionId, payload)
  const duplicateAck = await harness.waitForAck(actionId, 2)

  assert.equal(executions, 1)
  assert.equal(duplicateAck.output, 'Hello, world!\n')
  await harness.close()
})

test('deduplicates an in-flight terminal command by action id', async () => {
  let executions = 0
  let release: (() => void) | undefined
  const held = new Promise<void>((resolve) => {
    release = resolve
  })
  const harness = await startHarness(async () => {
    executions += 1
    await held
    return {
      status: 'done',
      exit_code: 0,
      output: 'Hello, world!\n',
    }
  })
  const actionId = randomUUID()
  const payload = {
    type: 'TerminalCommand',
    session_key: 'main',
    cwd: 'hello_world',
    command: '.\\main.exe',
    accepted_exit_codes: [0],
    expected_output: ['Hello, world!'],
    timeout_ms: 300000,
  }

  harness.sendActionPayload(actionId, payload)
  harness.sendActionPayload(actionId, payload)
  release?.()

  const first = await harness.waitForAck(actionId)
  const duplicate = await harness.waitForAck(actionId, 2)

  assert.equal(executions, 1)
  assert.equal(first.output, 'Hello, world!\n')
  assert.equal(duplicate.output, 'Hello, world!\n')
  await harness.close()
})

test('ignores a cancel for an unknown request id', async () => {
  let executions = 0
  const harness = await startHarness(async () => {
    executions += 1
    return { status: 'done' }
  })
  const actionId = randomUUID()

  harness.sendCancel(actionId)
  harness.sendAction(actionId, 'later action')
  const ack = await harness.waitForAck(actionId)

  assert.equal(executions, 1)
  assert.equal(ack.status, 'done')
  await harness.close()
})

test('deduplicates an in-flight workspace reset by request id', async () => {
  let resets = 0
  let release: (() => void) | undefined
  const held = new Promise<void>((resolve) => {
    release = resolve
  })
  const harness = await startHarness(
    async () => ({ status: 'done' }),
    async () => {
      resets += 1
      await held
      return { status: 'done' }
    },
  )
  const requestId = randomUUID()

  harness.sendWorkspaceReset(requestId, ['src'])
  harness.sendWorkspaceReset(requestId, ['src'])
  release?.()

  await harness.waitForAck(requestId)
  await harness.waitForAck(requestId, 2)

  assert.equal(resets, 1)
  await harness.close()
})

test('cancels a queued workspace reset before it starts', async () => {
  let resets = 0
  let started: (() => void) | undefined
  let release: (() => void) | undefined
  const firstStarted = new Promise<void>((resolve) => {
    started = resolve
  })
  const held = new Promise<void>((resolve) => {
    release = resolve
  })
  const harness = await startHarness(
    async () => ({ status: 'done' }),
    async () => {
      resets += 1
      if (resets === 1) {
        started?.()
        await held
      }
      return { status: 'done' }
    },
  )
  const first = randomUUID()
  const second = randomUUID()

  harness.sendWorkspaceReset(first, ['first'])
  await firstStarted
  harness.sendWorkspaceReset(second, ['second'])
  harness.sendCancel(second)
  release?.()

  const firstAck = await harness.waitForAck(first)
  const secondAck = await harness.waitForAck(second)

  assert.equal(resets, 1)
  assert.equal(firstAck.status, 'done')
  assert.equal(secondAck.status, 'cancelled')
  await harness.close()
})

test('closes tabs for workspace reset paths and acknowledges completion', async () => {
  let received: string[] = []
  const harness = await startHarness(
    async () => ({ status: 'done' }),
    async ({ paths }) => {
      received = paths
      return { status: 'done' }
    },
  )
  const messageId = randomUUID()

  harness.sendWorkspaceReset(messageId, ['src', 'hello_cargo'])
  const ack = await harness.waitForAck(messageId)

  assert.deepEqual(received, ['src', 'hello_cargo'])
  assert.equal(ack.status, 'done')
  await harness.close()
})

test('passes terminal-only cleanup without closing tabs', async () => {
  let received: { paths: string[]; close_tabs?: boolean } | undefined
  const harness = await startHarness(
    async () => ({ status: 'done' }),
    async (payload) => {
      received = payload
      return { status: 'done' }
    },
  )
  const messageId = randomUUID()

  harness.sendWorkspaceReset(messageId, ['ch/unit'], false)
  const ack = await harness.waitForAck(messageId)

  assert.deepEqual(received, {
    paths: ['ch/unit'],
    close_tabs: false,
  })
  assert.equal(ack.status, 'done')
  await harness.close()
})

test('passes excluded descendant workspace paths through', async () => {
  let received:
    | { paths: string[]; exclude_paths?: string[]; close_tabs?: boolean }
    | undefined
  const harness = await startHarness(
    async () => ({ status: 'done' }),
    async (payload) => {
      received = payload
      return { status: 'done' }
    },
  )
  const messageId = randomUUID()

  harness.sendWorkspaceReset(messageId, ['ch/parent'], true, [
    'ch/parent/child',
  ])
  const ack = await harness.waitForAck(messageId)

  assert.deepEqual(received, {
    paths: ['ch/parent'],
    exclude_paths: ['ch/parent/child'],
    close_tabs: true,
  })
  assert.equal(ack.status, 'done')
  await harness.close()
})

test('allows an empty workspace reset when only managed terminals need closing', async () => {
  let received: string[] | undefined
  const harness = await startHarness(
    async () => ({ status: 'done' }),
    async ({ paths }) => {
      received = paths
      return { status: 'done' }
    },
  )
  const messageId = randomUUID()

  harness.sendWorkspaceReset(messageId, [])
  const ack = await harness.waitForAck(messageId)

  assert.deepEqual(received, [])
  assert.equal(ack.status, 'done')
  await harness.close()
})

async function waitFor(
  predicate: () => boolean,
  timeoutMs = 1000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() >= deadline) {
      throw new Error('timed out waiting for adapter message')
    }
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
}
