import * as path from 'node:path'

import { directoryIsWithin } from './workspacePaths'

export interface DirtyDocumentLike {
  isDirty: boolean
  uri: {
    scheme: string
    fsPath: string
  }
}

export function dirtyDocumentsUnder<T extends DirtyDocumentLike>(
  root: string | undefined,
  documents: readonly T[],
): T[] {
  if (!root) {
    return []
  }
  return documents.filter((document) => {
    if (!document.isDirty || document.uri.scheme !== 'file') {
      return false
    }
    return directoryIsWithin(root, document.uri.fsPath)
  })
}

export function resolveUnitCheckCwd(
  unitRoot: string,
  cwd: string,
): string | undefined {
  const target = path.resolve(unitRoot, cwd)
  const relative = path.relative(unitRoot, target)
  if (
    relative === '..' ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    return undefined
  }
  return target
}
