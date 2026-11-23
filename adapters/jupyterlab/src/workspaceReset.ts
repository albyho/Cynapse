import { JupyterLabFacadeError } from './actions'
import type { AckPayload } from './protocol'

/** One workspace path is always the Course Environment; never close into it. */
const COURSE_ENVIRONMENT_DIR = '.venv'

export interface WorkspaceResetRequest {
  paths: string[]
  excludePaths: string[]
  closeTabs: boolean
  /** When non-empty, close only these managed terminal sessions. */
  sessionKeys?: string[]
}

/**
 * The reset seam between the Adapter and JupyterLab. Notebook paths are
 * workspace-relative and `/`-separated, like JupyterLab document paths.
 */
export interface JupyterLabResetFacade {
  openNotebookPaths(): readonly string[]
  closeNotebook(path: string, signal: AbortSignal): Promise<void>
  shutdownKernel(path: string, signal: AbortSignal): Promise<void>
  closeTerminals(
    paths: string[],
    excludePaths: string[],
    sessionKeys: readonly string[],
    signal: AbortSignal,
  ): Promise<void>
}

export interface WorkspaceResetContext {
  signal: AbortSignal
}

export type WorkspaceResetHandler = (
  request: WorkspaceResetRequest,
  context: WorkspaceResetContext,
) => Promise<AckPayload>

/**
 * Reset ACKs only after Notebooks are closed and their Kernels are shut down.
 * Course Reset passes `.`; Unit Reset excludes descendant Units, so resources
 * and other Units stay open.
 */
export function createWorkspaceResetHandler(
  facade: JupyterLabResetFacade,
): WorkspaceResetHandler {
  return async (request, { signal }) => {
    try {
      if (!request.closeTabs) {
        await facade.closeTerminals(
          request.paths,
          request.excludePaths,
          request.sessionKeys ?? [],
          signal,
        )
        return cancelledWhenAborted(signal)
      }

      const notebooks = notebooksForReset(
        facade.openNotebookPaths(),
        request.paths,
        request.excludePaths,
      )
      for (const path of notebooks) {
        if (signal.aborted) {
          return { status: 'cancelled' }
        }
        await facade.closeNotebook(path, signal)
      }

      const remaining = notebooksForReset(
        facade.openNotebookPaths(),
        request.paths,
        request.excludePaths,
      )
      if (remaining.length > 0) {
        return {
          status: 'error',
          code: 'notebook_close_incomplete',
          message: `Notebooks could not be closed: ${remaining.join(', ')}`,
        }
      }

      for (const path of notebooks) {
        if (signal.aborted) {
          return { status: 'cancelled' }
        }
        await facade.shutdownKernel(path, signal)
      }
      return cancelledWhenAborted(signal)
    } catch (error) {
      if (signal.aborted) {
        return { status: 'cancelled' }
      }
      if (error instanceof JupyterLabFacadeError) {
        return { status: 'error', code: error.code, message: error.message }
      }
      return {
        status: 'error',
        code: 'workspace_reset_failed',
        message: error instanceof Error ? error.message : String(error),
      }
    }
  }
}

function notebooksForReset(
  openPaths: readonly string[],
  paths: string[],
  excludePaths: string[],
): string[] {
  const included = paths.map(normalizeWorkspacePath)
  const excluded = excludePaths.map(normalizeWorkspacePath)
  return [...new Set(openPaths)].filter((path) => {
    const candidate = normalizeWorkspacePath(path)
    return (
      !isCourseEnvironment(candidate) &&
      included.some((base) => isWithin(candidate, base)) &&
      !excluded.some((base) => isWithin(candidate, base))
    )
  })
}

function normalizeWorkspacePath(path: string): string {
  return path
    .replace(/\\/g, '/')
    .replace(/^\.\/+/, '')
    .replace(/\/+$/, '')
}

function isWithin(candidate: string, base: string): boolean {
  if (base === '' || base === '.') {
    return candidate !== '' && candidate !== '.'
  }
  return candidate === base || candidate.startsWith(`${base}/`)
}

function isCourseEnvironment(path: string): boolean {
  return path === COURSE_ENVIRONMENT_DIR
    ? true
    : path.startsWith(`${COURSE_ENVIRONMENT_DIR}/`)
}

function cancelledWhenAborted(signal: AbortSignal): AckPayload {
  return signal.aborted ? { status: 'cancelled' } : { status: 'done' }
}
