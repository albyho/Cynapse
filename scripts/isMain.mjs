import path from 'node:path'
import { pathToFileURL } from 'node:url'

/** True when `moduleUrl` — the caller's `import.meta.url` — is the entry point. */
export function isMain(moduleUrl) {
  return (
    Boolean(process.argv[1]) &&
    moduleUrl === pathToFileURL(path.resolve(process.argv[1])).href
  )
}
