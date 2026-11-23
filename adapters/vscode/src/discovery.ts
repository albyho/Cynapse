import * as fs from 'node:fs'
import * as path from 'node:path'

export interface Discovery {
  /** Optional; missing or empty means "do not open a workspace". */
  workspace?: string
  port: number
  token: string
  pid: number
  started_at: number
}

export function globalRuntimePath(): string {
  const base =
    process.env.LOCALAPPDATA ?? process.env.APPDATA ?? process.env.HOME
  return base
    ? path.join(base, 'Cynapse', 'Client', 'runtime.json')
    : path.join('.', 'Cynapse', 'Client', 'runtime.json')
}

export function parseDiscovery(raw: string): Discovery | undefined {
  try {
    const parsed = JSON.parse(raw) as Partial<Discovery>

    if (
      typeof parsed.port !== 'number' ||
      parsed.port <= 0 ||
      parsed.port > 65535 ||
      typeof parsed.token !== 'string' ||
      parsed.token.length === 0 ||
      typeof parsed.pid !== 'number' ||
      parsed.pid <= 0 ||
      typeof parsed.started_at !== 'number' ||
      (parsed.workspace !== undefined && typeof parsed.workspace !== 'string')
    ) {
      return undefined
    }

    return parsed as Discovery
  } catch {
    return undefined
  }
}

export function readDiscovery(): Discovery | undefined {
  try {
    return parseDiscovery(fs.readFileSync(globalRuntimePath(), 'utf8'))
  } catch {
    return undefined
  }
}

export function resolveDiscovery(): Discovery | undefined {
  const discovery = readDiscovery()
  // The Client owns this file. Never unlink a stale discovery here: a new
  // Client can replace it between the read and the unlink.
  return discovery && isProcessAlive(discovery.pid) ? discovery : undefined
}

export function isProcessAlive(pid: number): boolean {
  if (pid <= 0) {
    return false
  }

  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === 'EPERM'
  }
}
