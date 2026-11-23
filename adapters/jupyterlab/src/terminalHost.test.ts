import assert from 'node:assert/strict'
import { test as nodeTest } from 'node:test'

import {
  createJupyterServerTerminalHost,
  type JupyterWebSocketLike,
} from './terminalFacade'

function test(name: string, run: () => Promise<void> | void): void {
  nodeTest(name, { timeout: 5_000 }, run)
}

class FakeWebSocket implements JupyterWebSocketLike {
  readyState = 0
  readonly sent: string[] = []
  private readonly listeners = new Map<string, Set<(event: unknown) => void>>()

  addEventListener(type: string, listener: (event: unknown) => void): void {
    const listeners = this.listeners.get(type) ?? new Set()
    listeners.add(listener)
    this.listeners.set(type, listeners)
  }

  removeEventListener(type: string, listener: (event: unknown) => void): void {
    this.listeners.get(type)?.delete(listener)
  }

  send(data: string): void {
    this.sent.push(data)
  }

  close(): void {
    this.readyState = 3
    this.emit('close', {})
  }

  open(): void {
    this.readyState = 1
    this.emit('open', {})
  }

  emit(type: string, event: unknown): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event)
    }
  }
}

interface FetchCall {
  url: string
  init: RequestInit | undefined
}

function fakeResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
  } as Response
}

function fakeFetch(
  calls: FetchCall[],
  respond: (url: string, init: RequestInit | undefined) => Response,
): typeof fetch {
  return (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input)
    calls.push({ url, init })
    return respond(url, init)
  }) as typeof fetch
}

test('creates a Jupyter terminal, opens its WebSocket, and forwards stdout', async () => {
  const calls: FetchCall[] = []
  const sockets: FakeWebSocket[] = []
  const host = createJupyterServerTerminalHost({
    baseUrl: 'http://127.0.0.1:8888/',
    fetch: fakeFetch(calls, (url) => {
      assert.match(url, /\/api\/terminals$/)
      return fakeResponse(201, { name: 'terminal-1' })
    }),
    createWebSocket: (url) => {
      assert.equal(url, 'ws://127.0.0.1:8888/terminals/websocket/terminal-1')
      const socket = new FakeWebSocket()
      sockets.push(socket)
      return socket
    },
  })

  const pending = host.create('ch/unit')
  await waitFor(() => sockets.length === 1)
  sockets[0].open()
  const connection = await pending

  assert.equal(connection.name, 'terminal-1')
  assert.equal(calls[0].init?.method, 'POST')
  assert.deepEqual(JSON.parse(String(calls[0].init?.body)), {
    cwd: 'ch/unit',
  })

  const output: string[] = []
  connection.onOutput((chunk) => output.push(chunk))
  sockets[0].emit('message', {
    data: JSON.stringify(['stdout', 'hello\r\n']),
  })
  sockets[0].emit('message', { data: JSON.stringify(['set_size', 80, 24]) })
  connection.send('python --version\r')

  assert.deepEqual(output, ['hello\r\n'])
  assert.deepEqual(sockets[0].sent, [
    JSON.stringify(['stdin', 'python --version\r']),
  ])
  connection.dispose()
  assert.equal(connection.closed, true)
})

test('completion markers distinguish files, missing paths, and directories', async () => {
  const host = createJupyterServerTerminalHost({
    baseUrl: 'http://127.0.0.1:8888',
    fetch: fakeFetch([], (url) => {
      if (url.includes('/api/contents/ch/unit/missing')) {
        return fakeResponse(404, null)
      }
      if (url.includes('/api/contents/ch/unit/directory')) {
        return fakeResponse(200, { type: 'directory' })
      }
      return fakeResponse(200, { type: 'file' })
    }),
    createWebSocket: () => new FakeWebSocket(),
  })

  assert.equal(await host.fileExists('ch/unit/done.txt'), true)
  assert.equal(await host.fileExists('ch/unit/missing'), false)
  await assert.rejects(
    () => host.fileExists('ch/unit/directory'),
    /is not a file/,
  )
})

async function waitFor(
  predicate: () => boolean,
  timeoutMs = 1000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() >= deadline) {
      throw new Error('timed out waiting for the fake WebSocket')
    }
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
}
