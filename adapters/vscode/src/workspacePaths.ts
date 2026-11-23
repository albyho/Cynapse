import * as fs from 'node:fs/promises'
import * as path from 'node:path'

import { ApplyError } from './applyModel'

// `path.relative` applies the host filesystem's case rules. Tests can inject
// `path.win32` / `path.posix` so both semantics run on any host.
export type PathApi = Pick<typeof path, 'resolve' | 'relative' | 'isAbsolute' | 'sep'>

export function pathIsWithin(
  root: string,
  candidate: string,
  pathApi: PathApi = path,
): boolean {
  const relative = pathApi.relative(
    pathApi.resolve(root),
    pathApi.resolve(candidate),
  )
  if (relative === '') {
    return true
  }
  if (pathApi.isAbsolute(relative)) {
    return false
  }
  const [first] = relative.split(pathApi.sep)
  return first !== '..'
}

export function directoryIsWithin(root: string, candidate: string): boolean {
  return pathIsWithin(root, candidate)
}

export function samePath(
  left: string,
  right: string,
  pathApi: PathApi = path,
): boolean {
  return (
    pathApi.relative(pathApi.resolve(left), pathApi.resolve(right)) === ''
  )
}

// Realpath of `target`, or of its nearest existing ancestor plus the rest.
export async function canonicalWorkspacePath(target: string): Promise<string> {
  try {
    return await fs.realpath(target)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw error
    }
    const parent = path.dirname(target)
    if (parent === target) {
      return target
    }
    return path.join(await canonicalWorkspacePath(parent), path.basename(target))
  }
}

// Resolves an absolute target through the real filesystem so a symlinked
// directory cannot escape the workspace.
export async function resolveRealWorkspacePath(
  workspaceRoot: string,
  target: string,
): Promise<string> {
  const root = await fs.realpath(workspaceRoot)
  const canonical = await canonicalWorkspacePath(path.resolve(target))
  if (!directoryIsWithin(root, canonical)) {
    throw new ApplyError(
      'path_outside_workspace',
      `${target} is outside the workspace`,
    )
  }
  return canonical
}
