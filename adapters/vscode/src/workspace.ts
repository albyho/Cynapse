import { samePath } from './workspacePaths'

export function workspaceRootFromConfig(
  configuredWorkspaceRoot: string | undefined,
): string | undefined {
  return configuredWorkspaceRoot?.trim() ? configuredWorkspaceRoot : undefined
}

export function shouldOpenWorkspace(
  currentWorkspace: string | undefined,
  targetWorkspace: string | undefined,
): boolean {
  if (!targetWorkspace?.trim()) {
    return false
  }
  if (!currentWorkspace?.trim()) {
    return true
  }

  return !samePath(currentWorkspace, targetWorkspace)
}
