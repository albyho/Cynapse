import assert from 'node:assert/strict'
import { once } from 'node:events'
import type { AddressInfo } from 'node:net'
import { after, test as nodeTest } from 'node:test'

import WebSocket, { WebSocketServer } from 'ws'

import { randomUUID } from 'node:crypto'

import {
  DEFAULT_ADAPTER_PORT,
  JupyterLabAdapterConnection,
  type CheckHandler,
  type JupyterLabAdapterStatus,
} from './adapterConnection'
import type { ActionHandler } from './actions'
import type { AckPayload, Envelope } from './protocol'
import { PROTOCOL_VERSION } from './protocol'
import type { WorkspaceResetHandler } from './workspaceReset'

const TEST_TIMEOUT_MS = 5_000
const activeHarnesses = new Set<Harness>()
const activeAdapters = new Set<JupyterLabAdapterConnection>()

/** Every test is bounded so a missing ACK fails instead of hanging the runner. */
function test(name: string, run: () => Promise<void> | void): void {
  nodeTest(name, { timeout: TEST_TIMEOUT_MS }, run)
}

/** A failed assertion can skip the local close; the hook still frees sockets. */
after(async () => {
  for (const adapter of activeAdapters) {
    adapter.dispose()
  }
  await Promise.all(
    [...activeHarnesses].map((harness) =>
      harness.close().catch(() => undefined),
    ),
  )
})

interface Harness {
  port: number
  messages: Envelope[]
  sockets: WebSocket[]
  waitForMessage: (type: string, occurrence?: number) => Promise<Envelope>
  waitForConnection: (occurrence?: number) => Promise<WebSocket>
  replyHelloAck: (occurrence?: number) => void
  replyRejectedHandshake: (occurrence?: number) => void
  sendAction: (id: string, payload: Record<string, unknown>) => void
  sendCheck: (id: string, payload: Record<string, unknown>) => void
  sendWorkspaceReset: (id: string, payload: Record<string, unknown>) => void
  sendCancel: (targetId: string) => void
  waitForAck: (id: string, occurrence?: number) => Promise<AckPayload>
  disconnectAdapter: () => Promise<void>
  close: () => Promise<void>
}

async function startHarness(onAction?: ActionHandler): Promise<Harness> {
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  await once(server, 'listening')
  const address = server.address() as AddressInfo
  const messages: Envelope[] = []
  const sockets: WebSocket[] = []

  server.on('connection', (socket) => {
    sockets.push(socket)
    socket.on('message', (data) => {
      messages.push(JSON.parse(data.toString()) as Envelope)
    })
  })

  const sendMessage = (
    type: Envelope['type'],
    id: string,
    payload: Record<string, unknown>,
  ): void => {
    const socket = sockets.at(-1)
    assert.ok(socket)
    socket.send(
      JSON.stringify({
        v: PROTOCOL_VERSION,
        id,
        type,
        payload,
      } satisfies Envelope),
    )
  }

  const waitForMessage = async (
    type: string,
    occurrence = 1,
  ): Promise<Envelope> => {
    const matching = () => messages.filter((message) => message.type === type)
    await waitFor(() => matching().length >= occurrence)
    const message = matching()[occurrence - 1]
    assert.ok(message)
    return message
  }

  let closed = false
  const harness: Harness = {
    port: address.port,
    messages,
    sockets,
    waitForMessage,
    waitForConnection: async (occurrence = 1) => {
      await waitFor(() => sockets.length >= occurrence)
      const socket = sockets[occurrence - 1]
      assert.ok(socket)
      return socket
    },
    replyHelloAck: (occurrence = 1) => {
      const socket = sockets[occurrence - 1]
      assert.ok(socket)
      socket.send(
        JSON.stringify({
          v: PROTOCOL_VERSION,
          id: messageId(occurrence),
          type: 'hello_ack',
          payload: { adapter: 'jupyterlab' },
        } satisfies Envelope),
      )
    },
    replyRejectedHandshake: (occurrence = 1) => {
      const socket = sockets[occurrence - 1]
      assert.ok(socket)
      socket.send(
        JSON.stringify({
          v: PROTOCOL_VERSION,
          id: messageId(occurrence),
          type: 'ack',
          payload: {
            status: 'error',
            code: 'invalid_origin',
            message: 'unexpected adapter origin',
          },
        } satisfies Envelope),
      )
    },
    sendAction: (id, payload) => {
      sendMessage('action', id, payload)
    },
    sendCheck: (id, payload) => {
      sendMessage('check', id, payload)
    },
    sendWorkspaceReset: (id, payload) => {
      sendMessage('workspace_reset', id, payload)
    },
    sendCancel: (targetId) => {
      const socket = sockets.at(-1)
      assert.ok(socket)
      socket.send(
        JSON.stringify({
          v: PROTOCOL_VERSION,
          id: randomUUID(),
          type: 'cancel',
          payload: { target_id: targetId },
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
      const socket = sockets.at(-1)
      assert.ok(socket)
      if (socket.readyState === WebSocket.CLOSED) {
        return
      }
      const socketClosed = once(socket, 'close')
      socket.close()
      await withTimeout(
        socketClosed,
        1_000,
        'the adapter WebSocket did not close',
      )
    },
    close: async () => {
      if (closed) {
        return
      }
      closed = true
      activeHarnesses.delete(harness)
      const serverClosed = once(server, 'close')
      for (const socket of sockets) {
        socket.close()
      }
      server.close()
      try {
        await withTimeout(
          serverClosed,
          2_000,
          'the adapter test server did not close',
        )
      } finally {
        for (const socket of sockets) {
          socket.terminate()
        }
      }
    },
  }
  activeHarnesses.add(harness)
  return harness
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(message)), timeoutMs)
    timer.unref()
  })
  try {
    return await Promise.race([promise, timeout])
  } finally {
    if (timer) {
      clearTimeout(timer)
    }
  }
}

function messageId(occurrence: number): string {
  return `00000000-0000-4000-8000-00000000000${occurrence}`
}

function startAdapter(
  port: number,
  statuses: JupyterLabAdapterStatus[],
  onAction?: ActionHandler,
  onCheck?: CheckHandler,
  onWorkspaceReset?: WorkspaceResetHandler,
): JupyterLabAdapterConnection {
  const adapter = new JupyterLabAdapterConnection(
    { port, reconnectDelayMs: 10 },
    (status) => {
      statuses.push(status)
    },
    onAction,
    onCheck,
    onWorkspaceReset,
  )
  adapter.connect()
  activeAdapters.add(adapter)
  return adapter
}

test('connects to the fixed loopback control port and reports the adapter identity', async () => {
  assert.equal(DEFAULT_ADAPTER_PORT, 37654)
  const harness = await startHarness()
  const statuses: JupyterLabAdapterStatus[] = []
  const adapter = startAdapter(harness.port, statuses)

  const hello = await harness.waitForMessage('hello')
  assert.equal(hello.v, PROTOCOL_VERSION)
  assert.deepEqual(hello.payload, {
    token: '',
    role: 'adapter',
  })

  harness.replyHelloAck()
  await waitFor(() => statuses.some((status) => status.kind === 'connected'))
  assert.deepEqual(statuses.at(-1), {
    kind: 'connected',
    adapter: 'jupyterlab',
  })

  adapter.dispose()
  await harness.close()
})

test('reconnects and handshakes again after the Client connection closes', async () => {
  const harness = await startHarness()
  const statuses: JupyterLabAdapterStatus[] = []
  const adapter = startAdapter(harness.port, statuses)

  await harness.waitForMessage('hello', 1)
  harness.replyHelloAck(1)
  await waitFor(() => statuses.some((status) => status.kind === 'connected'))

  await harness.waitForConnection(1).then((socket) => socket.close())
  await waitFor(() => statuses.at(-1)?.kind === 'waiting')

  await harness.waitForMessage('hello', 2)
  harness.replyHelloAck(2)
  await waitFor(
    () => statuses.filter((status) => status.kind === 'connected').length >= 2,
  )

  adapter.dispose()
  await harness.close()
})

test('stops reconnecting when a newer page makes this one inactive', async () => {
  const harness = await startHarness()
  const statuses: JupyterLabAdapterStatus[] = []
  const adapter = startAdapter(harness.port, statuses)

  await harness.waitForMessage('hello', 1)
  harness.replyHelloAck(1)
  await waitFor(() => statuses.some((status) => status.kind === 'connected'))

  const socket = await harness.waitForConnection(1)
  socket.close(4001, 'Inactive: a newer JupyterLab page is active')
  await waitFor(() => statuses.at(-1)?.kind === 'inactive')
  assert.deepEqual(statuses.at(-1), {
    kind: 'inactive',
    message: 'Inactive: a newer JupyterLab page is active',
  })

  await new Promise((resolve) => setTimeout(resolve, 60))
  assert.equal(harness.sockets.length, 1)

  adapter.dispose()
  await harness.close()
})

test('stops reconnecting after the Client rejects the handshake', async () => {
  const harness = await startHarness()
  const statuses: JupyterLabAdapterStatus[] = []
  const adapter = startAdapter(harness.port, statuses)

  await harness.waitForMessage('hello', 1)
  harness.replyRejectedHandshake(1)
  await waitFor(() => statuses.some((status) => status.kind === 'error'))

  await new Promise((resolve) => setTimeout(resolve, 60))
  assert.equal(harness.sockets.length, 1)

  adapter.dispose()
  await harness.close()
})

async function connectedAdapter(
  harness: Harness,
  onAction?: ActionHandler,
  onCheck?: CheckHandler,
  onWorkspaceReset?: WorkspaceResetHandler,
): Promise<JupyterLabAdapterConnection> {
  const statuses: JupyterLabAdapterStatus[] = []
  const adapter = startAdapter(
    harness.port,
    statuses,
    onAction,
    onCheck,
    onWorkspaceReset,
  )
  await harness.waitForMessage('hello', 1)
  harness.replyHelloAck(1)
  await waitFor(() => statuses.some((status) => status.kind === 'connected'))
  return adapter
}

test('re-sends the ack for a duplicate action without executing it twice', async () => {
  const harness = await startHarness()
  let executions = 0
  const adapter = await connectedAdapter(harness, async () => {
    executions += 1
    return { status: 'done', output: 'ok\n' }
  })
  const actionId = randomUUID()
  const payload = {
    type: 'JupyterLabCellRun',
    path: 'lesson.ipynb',
    from_cell_id: 'cell-1',
    to_cell_id: 'cell-1',
    timeout_ms: 1000,
  }

  harness.sendAction(actionId, payload)
  const firstAck = await harness.waitForAck(actionId)
  harness.sendAction(actionId, payload)
  const secondAck = await harness.waitForAck(actionId, 2)

  assert.equal(executions, 1)
  assert.deepEqual(firstAck, { status: 'done', output: 'ok\n' })
  assert.deepEqual(secondAck, firstAck)
  adapter.dispose()
  await harness.close()
})

test('serializes actions so a second Cell never runs while one is active', async () => {
  const harness = await startHarness()
  const order: string[] = []
  let releaseFirst!: () => void
  const firstGate = new Promise<void>((resolve) => {
    releaseFirst = resolve
  })
  const adapter = await connectedAdapter(
    harness,
    async (envelope, { signal }) => {
      order.push(envelope.payload.type as string)
      if (order.length === 1) {
        await firstGate
      }
      return signal.aborted ? { status: 'cancelled' } : { status: 'done' }
    },
  )
  const first = randomUUID()
  const second = randomUUID()

  harness.sendAction(first, {
    type: 'JupyterLabCellRun',
    path: 'lesson.ipynb',
    from_cell_id: 'cell-1',
    to_cell_id: 'cell-1',
  })
  await waitFor(() => order.length === 1)
  harness.sendAction(second, {
    type: 'JupyterLabOpenNotebook',
    path: 'lesson.ipynb',
  })
  await new Promise((resolve) => setTimeout(resolve, 30))
  assert.deepEqual(order, ['JupyterLabCellRun'])

  releaseFirst()
  await harness.waitForAck(first)
  await harness.waitForAck(second)
  assert.deepEqual(order, ['JupyterLabCellRun', 'JupyterLabOpenNotebook'])
  adapter.dispose()
  await harness.close()
})

test('acknowledges workspace_reset only after the cleanup handler completes', async () => {
  const harness = await startHarness()
  let startCleanup!: () => void
  let releaseCleanup!: () => void
  const started = new Promise<void>((resolve) => {
    startCleanup = resolve
  })
  const released = new Promise<void>((resolve) => {
    releaseCleanup = resolve
  })
  const requests: unknown[] = []
  const adapter = await connectedAdapter(
    harness,
    undefined,
    undefined,
    async (request) => {
      requests.push(request)
      startCleanup()
      await released
      return { status: 'done' }
    },
  )
  const id = randomUUID()

  harness.sendWorkspaceReset(id, {
    paths: ['ch/unit'],
    exclude_paths: ['ch/unit/child'],
    close_tabs: true,
  })
  await started
  await new Promise((resolve) => setTimeout(resolve, 30))
  assert.equal(
    harness.messages.filter(
      (message) => message.type === 'ack' && message.id === id,
    ).length,
    0,
    'no ACK before Notebook cleanup finishes',
  )

  releaseCleanup()
  assert.deepEqual(await harness.waitForAck(id), { status: 'done' })
  assert.deepEqual(requests, [
    {
      paths: ['ch/unit'],
      excludePaths: ['ch/unit/child'],
      closeTabs: true,
    },
  ])
  adapter.dispose()
  await harness.close()
})

test('rejects a malformed workspace_reset payload', async () => {
  const harness = await startHarness()
  const adapter = await connectedAdapter(harness)
  const id = randomUUID()

  harness.sendWorkspaceReset(id, { paths: [''] })

  assert.deepEqual(await harness.waitForAck(id), {
    status: 'error',
    code: 'invalid_workspace_reset',
    message: 'Invalid workspace reset payload',
  })
  adapter.dispose()
  await harness.close()
})

test('aborts the active action when a matching cancel arrives', async () => {
  const harness = await startHarness()
  const adapter = await connectedAdapter(
    harness,
    async (_envelope, { signal }) =>
      new Promise<AckPayload>((resolve) => {
        signal.addEventListener(
          'abort',
          () => resolve({ status: 'cancelled' }),
          {
            once: true,
          },
        )
      }),
  )
  const actionId = randomUUID()

  harness.sendAction(actionId, {
    type: 'JupyterLabCellRun',
    path: 'lesson.ipynb',
    from_cell_id: 'cell-1',
    to_cell_id: 'cell-1',
  })
  await new Promise((resolve) => setTimeout(resolve, 20))
  harness.sendCancel(actionId)

  assert.deepEqual(await harness.waitForAck(actionId), { status: 'cancelled' })
  adapter.dispose()
  await harness.close()
})

test('aborts the active action when the Client connection closes', async () => {
  const harness = await startHarness()
  let aborted!: () => void
  const abortSeen = new Promise<void>((resolve) => {
    aborted = resolve
  })
  const adapter = await connectedAdapter(
    harness,
    async (_envelope, { signal }) =>
      new Promise<AckPayload>((resolve) => {
        signal.addEventListener(
          'abort',
          () => {
            aborted()
            resolve({ status: 'cancelled' })
          },
          { once: true },
        )
      }),
  )

  harness.sendAction(randomUUID(), {
    type: 'JupyterLabCellRun',
    path: 'lesson.ipynb',
    from_cell_id: 'cell-1',
    to_cell_id: 'cell-1',
  })
  await new Promise((resolve) => setTimeout(resolve, 20))
  await harness.disconnectAdapter()
  await Promise.race([
    abortSeen,
    new Promise((_, reject) =>
      setTimeout(() => reject(new Error('active action was not aborted')), 500),
    ),
  ])
  adapter.dispose()
  await harness.close()
})

test('routes a challenge check through the check handler', async () => {
  const harness = await startHarness()
  const checks: string[] = []
  const adapter = await connectedAdapter(
    harness,
    undefined,
    async (envelope) => {
      checks.push(envelope.payload.command as string)
      return { status: 'done', exit_code: 0, output: 'ok\n' }
    },
  )
  const actionId = randomUUID()

  harness.sendCheck(actionId, {
    type: 'COMMAND',
    unit_path: 'ch/unit',
    session_key: 'ch/unit::main',
    cwd: '.',
    command: 'python check.py',
  })

  assert.deepEqual(await harness.waitForAck(actionId), {
    status: 'done',
    exit_code: 0,
    output: 'ok\n',
  })
  assert.deepEqual(checks, ['python check.py'])
  adapter.dispose()
  await harness.close()
})

test('routes terminal cleanup through the workspace reset handler', async () => {
  const harness = await startHarness()
  const resets: unknown[] = []
  const adapter = await connectedAdapter(
    harness,
    undefined,
    undefined,
    async (payload) => {
      resets.push(payload)
      return { status: 'done' }
    },
  )
  const actionId = randomUUID()

  harness.sendWorkspaceReset(actionId, {
    paths: ['ch/unit'],
    exclude_paths: ['ch/unit/keep'],
    close_tabs: false,
  })

  assert.deepEqual(await harness.waitForAck(actionId), { status: 'done' })
  assert.deepEqual(resets, [
    {
      paths: ['ch/unit'],
      excludePaths: ['ch/unit/keep'],
      closeTabs: false,
    },
  ])
  adapter.dispose()
  await harness.close()
})

test('rejects an invalid workspace reset payload without calling the handler', async () => {
  const harness = await startHarness()
  let calls = 0
  const adapter = await connectedAdapter(
    harness,
    undefined,
    undefined,
    async () => {
      calls += 1
      return { status: 'done' }
    },
  )
  const actionId = randomUUID()

  harness.sendWorkspaceReset(actionId, { paths: 'ch/unit' })

  const ack = await harness.waitForAck(actionId)
  assert.equal(ack.status, 'error')
  assert.equal(ack.code, 'invalid_workspace_reset')
  assert.equal(calls, 0)
  adapter.dispose()
  await harness.close()
})

async function waitFor(
  predicate: () => boolean,
  timeoutMs = 1000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() >= deadline) {
      throw new Error('timed out waiting for the adapter connection state')
    }
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
}
