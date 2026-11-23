import * as path from 'node:path'

import { ApplyError, type ApplyOutcome } from './apply'
import {
  canonicalWorkspacePath,
  directoryIsWithin,
  samePath,
} from './workspacePaths'

export type TargetKind = 'missing' | 'file' | 'directory'

export interface CreateDirectoryAction {
  type: 'CreateDirectory'
  path: string
  create_parents?: boolean
}

export interface CreateFileAction {
  type: 'CreateFile'
  path: string
  create_parents?: boolean
}

export type MissingTargetPolicy = 'error' | 'ignore'

export interface DeleteFileAction {
  type: 'DeleteFile'
  path: string
  if_missing?: MissingTargetPolicy
}

export interface DeleteDirectoryAction {
  type: 'DeleteDirectory'
  path: string
  recursive?: boolean
  if_missing?: MissingTargetPolicy
}

export type FileSystemAction =
  | CreateDirectoryAction
  | CreateFileAction
  | DeleteFileAction
  | DeleteDirectoryAction

export interface FileSystemFacade {
  inspect(path: string): Promise<TargetKind>
  createDirectory(path: string, createParents: boolean): Promise<void>
  createFile(path: string, createParents: boolean): Promise<void>
  deleteFile(path: string): Promise<void>
  directoryIsEmpty(path: string): Promise<boolean>
  deleteDirectory(path: string, recursive: boolean): Promise<void>
  confirmRecursiveDelete(path: string): Promise<boolean>
}

export function resolveWorkspacePath(
  workspaceRoot: string,
  requestedPath: string,
): string {
  if (!requestedPath.trim()) {
    throw new ApplyError('invalid_path', 'The target path is empty')
  }

  const targetPath = path.resolve(workspaceRoot, requestedPath)
  const relative = path.relative(workspaceRoot, targetPath)
  if (
    relative === '..' ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    throw new ApplyError(
      'path_outside_workspace',
      `${requestedPath} is outside the workspace`,
    )
  }
  if (!relative) {
    throw new ApplyError(
      'workspace_root_not_mutable',
      'Filesystem actions cannot target the workspace root',
    )
  }
  return targetPath
}

export async function resolveUnitWorkspacePath(
  workspaceRoot: string,
  unitPath: string,
  requestedPath: string,
): Promise<string> {
  if (!unitPath.trim()) {
    throw new ApplyError('invalid_unit_path', 'The unit path is empty')
  }
  const targetPath = resolveWorkspacePath(workspaceRoot, requestedPath)
  const canonicalWorkspace = await canonicalWorkspacePath(workspaceRoot)
  const canonicalUnit = await canonicalWorkspacePath(
    path.resolve(workspaceRoot, unitPath),
  )
  if (!directoryIsWithin(canonicalWorkspace, canonicalUnit)) {
    throw new ApplyError(
      'invalid_unit_path',
      `${unitPath} is outside the workspace`,
    )
  }
  const canonicalTarget = await canonicalWorkspacePath(targetPath)
  if (!directoryIsWithin(canonicalUnit, canonicalTarget)) {
    throw new ApplyError(
      'path_outside_unit',
      `${requestedPath} is outside the unit workspace`,
    )
  }
  return targetPath
}

/**
 * Terminal cwd may be the Unit root, or the Course Workspace root for an
 * environment check that runs before the Unit workspace exists.
 */
export async function resolveTerminalWorkspacePath(
  workspaceRoot: string,
  unitPath: string,
  requestedPath: string,
): Promise<string> {
  if (!requestedPath.trim()) {
    throw new ApplyError('invalid_path', 'The target path is empty')
  }
  const targetPath = path.resolve(workspaceRoot, requestedPath)
  const canonicalWorkspace = await canonicalWorkspacePath(workspaceRoot)
  const canonicalTarget = await canonicalWorkspacePath(targetPath)
  if (!directoryIsWithin(canonicalWorkspace, canonicalTarget)) {
    throw new ApplyError(
      'path_outside_workspace',
      `${requestedPath} is outside the workspace`,
    )
  }
  if (samePath(canonicalWorkspace, canonicalTarget)) {
    return targetPath
  }
  const canonicalUnit = await canonicalWorkspacePath(
    path.resolve(workspaceRoot, unitPath),
  )
  if (!directoryIsWithin(canonicalUnit, canonicalTarget)) {
    throw new ApplyError(
      'path_outside_unit',
      `${requestedPath} is outside the unit workspace`,
    )
  }
  return targetPath
}

export async function resolveMutableUnitWorkspacePath(
  workspaceRoot: string,
  unitPath: string,
  requestedPath: string,
): Promise<string> {
  const targetPath = await resolveUnitWorkspacePath(
    workspaceRoot,
    unitPath,
    requestedPath,
  )
  if (samePath(targetPath, path.resolve(workspaceRoot, unitPath))) {
    throw new ApplyError(
      'workspace_root_not_mutable',
      'Filesystem actions cannot target the unit workspace root',
    )
  }
  return targetPath
}

function parentPath(target: string): string | undefined {
  const index = Math.max(target.lastIndexOf('/'), target.lastIndexOf('\\'))
  return index <= 0 ? undefined : target.slice(0, index)
}

export async function applyFileSystemAction(
  action: FileSystemAction,
  facade: FileSystemFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }

  if (action.type === 'CreateDirectory') {
    const createParents = action.create_parents ?? true
    const parent = parentPath(action.path)
    const parentKind = parent ? await facade.inspect(parent) : 'directory'
    if (parentKind === 'file') {
      throw new ApplyError(
        'parent_missing',
        `parent is not a directory for ${action.path}`,
      )
    }
    if (!createParents && parentKind !== 'directory') {
      throw new ApplyError(
        'parent_missing',
        `parent directory is missing for ${action.path}`,
      )
    }
    const existing = await facade.inspect(action.path)
    if (existing === 'directory') {
      return signal?.aborted ? 'cancelled' : 'done'
    }
    if (existing !== 'missing') {
      throw new ApplyError(
        'target_type_conflict',
        `${action.path} is not a directory`,
      )
    }
    if (signal?.aborted) {
      return 'cancelled'
    }
    await facade.createDirectory(action.path, createParents)
    return signal?.aborted ? 'cancelled' : 'done'
  }

  if (action.type === 'CreateFile') {
    const createParents = action.create_parents ?? true
    const parent = parentPath(action.path)
    const parentKind = parent ? await facade.inspect(parent) : 'directory'
    if (parentKind === 'file') {
      throw new ApplyError(
        'parent_missing',
        `parent is not a directory for ${action.path}`,
      )
    }
    if (!createParents && parentKind !== 'directory') {
      throw new ApplyError(
        'parent_missing',
        `parent directory is missing for ${action.path}`,
      )
    }
    const existing = await facade.inspect(action.path)
    if (existing === 'file') {
      return signal?.aborted ? 'cancelled' : 'done'
    }
    if (existing !== 'missing') {
      throw new ApplyError(
        'target_type_conflict',
        `${action.path} exists and is not a file`,
      )
    }
    if (signal?.aborted) {
      return 'cancelled'
    }
    await facade.createFile(action.path, createParents)
    return signal?.aborted ? 'cancelled' : 'done'
  }

  if (action.type === 'DeleteFile') {
    const existing = await facade.inspect(action.path)
    if (existing === 'missing') {
      return missingTarget(action.path, action.if_missing, signal)
    }
    if (existing !== 'file') {
      throw new ApplyError(
        'target_type_conflict',
        `${action.path} is not a file`,
      )
    }
    if (signal?.aborted) {
      return 'cancelled'
    }
    await facade.deleteFile(action.path)
    return signal?.aborted ? 'cancelled' : 'done'
  }

  if (action.type === 'DeleteDirectory') {
    const existing = await facade.inspect(action.path)
    if (existing === 'missing') {
      return missingTarget(action.path, action.if_missing, signal)
    }
    if (existing !== 'directory') {
      throw new ApplyError(
        'target_type_conflict',
        `${action.path} is not a directory`,
      )
    }

    const recursive = action.recursive ?? false
    if (!recursive && !(await facade.directoryIsEmpty(action.path))) {
      throw new ApplyError('directory_not_empty', `${action.path} is not empty`)
    }
    if (recursive) {
      if (!(await facade.confirmRecursiveDelete(action.path))) {
        return 'cancelled'
      }
    }
    if (signal?.aborted) {
      return 'cancelled'
    }
    await facade.deleteDirectory(action.path, recursive)
    return signal?.aborted ? 'cancelled' : 'done'
  }

  return 'done'
}

function missingTarget(
  path: string,
  policy: MissingTargetPolicy | undefined,
  signal?: AbortSignal,
): ApplyOutcome {
  if ((policy ?? 'error') === 'ignore') {
    return signal?.aborted ? 'cancelled' : 'done'
  }
  throw new ApplyError('target_missing', `${path} does not exist`)
}
