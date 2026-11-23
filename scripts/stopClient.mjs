import { execFileSync } from 'node:child_process'

// Skip the stop when the caller wants to keep the Client open; cargo may then
// fail to replace the executable while it is locked.
if (process.env.KEEP_CLIENT === '1') {
  process.exit(0)
}

if (process.platform === 'win32') {
  try {
    execFileSync('taskkill', ['/F', '/T', '/IM', 'cynapse-client.exe'], {
      stdio: 'ignore',
    })
  } catch {
    // No running Client, or it already exited.
  }
}
