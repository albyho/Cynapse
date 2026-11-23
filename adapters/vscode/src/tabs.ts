import * as path from 'node:path'

import { directoryIsWithin, samePath } from './workspacePaths'

interface UriLike {
  fsPath: string
}

export interface TabLike {
  input: unknown
}

export interface TabGroupLike<T extends TabLike = TabLike> {
  tabs: readonly T[]
  viewColumn?: number
}

export interface MatchedTab<
  T extends TabLike,
  G extends TabGroupLike<T> = TabGroupLike<T>,
> {
  tab: T
  group: G
}

/** OpenPreview 打开时记下的 tab 对象与文件完整路径。 */
export interface RecordedPreview<T extends TabLike> {
  tab: T
  filePath: string
}

function filePathsFromInput(input: unknown): string[] {
  if (!input || typeof input !== 'object') {
    return []
  }
  const candidate = input as {
    uri?: UriLike
    original?: UriLike
    modified?: UriLike
  }
  return [
    candidate.uri?.fsPath,
    candidate.original?.fsPath,
    candidate.modified?.fsPath,
  ].filter((value): value is string => typeof value === 'string')
}

function resetTargets(
  workspaceRoot: string | undefined,
  resetPaths: readonly string[],
): string[] {
  if (!workspaceRoot || resetPaths.length === 0) {
    return []
  }
  const root = path.resolve(workspaceRoot)
  // "." / 工作区根本身也是合法的重置目标：此时整个工作区内的 tab 都应关闭。
  return resetPaths
    .map((relative) => path.resolve(root, relative))
    .filter((target) => directoryIsWithin(root, target))
}

// 内置 Markdown 预览是 TabInputWebview，viewType 为 "markdown.preview"；
// 其它形态（custom editor、不同 VS Code 版本）也带 markdown + preview 字样。
const MARKDOWN_PREVIEW_VIEW_TYPE = /markdown.*preview|preview.*markdown/i

/**
 * 只凭 tab.input 的结构判断是否为 Markdown 预览：不带 uri、带 viewType。
 * 不使用标题——不同单元的 INTRO.md 标题完全相同，无法据此区分归属。
 */
export function isMarkdownPreviewViewType(viewType: string): boolean {
  return MARKDOWN_PREVIEW_VIEW_TYPE.test(viewType)
}

/** Remove records whose tab is no longer open. User-closed previews must not
 * keep `openedPreviews` non-empty, or workspace reset treats lost records as
 * present and stops cleaning untracked previews. */
export function pruneRecordedPreviews<T extends TabLike>(
  recorded: RecordedPreview<T>[],
  liveTabs: readonly T[],
): void {
  const live = new Set(liveTabs)
  for (let index = recorded.length - 1; index >= 0; index -= 1) {
    if (!live.has(recorded[index]!.tab)) {
      recorded.splice(index, 1)
    }
  }
}

export function isMarkdownPreviewInput(input: unknown): boolean {
  if (!input || typeof input !== 'object') {
    return false
  }
  const candidate = input as {
    viewType?: unknown
    uri?: unknown
    original?: unknown
    modified?: unknown
  }
  if (candidate.uri || candidate.original || candidate.modified) {
    return false
  }
  return (
    typeof candidate.viewType === 'string' &&
    isMarkdownPreviewViewType(candidate.viewType)
  )
}

/** 按文件路径精确匹配 tab：同一文件在多个编辑器组、diff 两侧都算。 */
export function tabsForFile<G extends TabGroupLike>(
  groups: readonly G[],
  filePath: string,
): MatchedTab<G['tabs'][number], G>[] {
  return groups.flatMap((group) =>
    group.tabs
      .filter((tab) =>
        filePathsFromInput(tab.input).some((candidate) =>
          samePath(candidate, filePath),
        ),
      )
      .map((tab) => ({ tab, group })),
  )
}

export function tabsForWorkspaceReset<
  T extends TabLike,
  G extends TabGroupLike<T>,
>(
  workspaceRoot: string | undefined,
  resetPaths: readonly string[],
  groups: readonly G[],
  recordedPreviews: readonly RecordedPreview<T>[] = [],
  excludePaths: readonly string[] = [],
): MatchedTab<T, G>[] {
  const targets = resetTargets(workspaceRoot, resetPaths)
  if (targets.length === 0) {
    return []
  }
  // The Client deletes the reset scope after this ACK, so close every tab in
  // scope now; `exclude_paths` protects descendant Units that stay on disk.
  const excludedTargets = resetTargets(workspaceRoot, excludePaths)
  const isExcluded = (filePath: string): boolean =>
    excludedTargets.some((target) => directoryIsWithin(target, filePath))
  // 仅把“位于本次重置目标范围内”的已记录预览纳入候选；精确匹配靠 tab
  // 对象引用，同名文件不会串台。
  const recordedByTab = new Map<unknown, string>()
  for (const entry of recordedPreviews) {
    const filePath = path.resolve(entry.filePath)
    if (
      targets.some((target) => directoryIsWithin(target, filePath)) &&
      !isExcluded(filePath)
    ) {
      recordedByTab.set(entry.tab, filePath)
    }
  }
  // 扩展重载后记录会丢失，此时无法归属任何预览，重置时统一清掉。
  const hasRecords = recordedPreviews.length > 0

  const matchingTabs: MatchedTab<T, G>[] = []
  for (const group of groups) {
    for (const tab of group.tabs) {
      const filePaths = filePathsFromInput(tab.input).map((filePath) =>
        path.resolve(filePath),
      )
      const isPreview = isMarkdownPreviewInput(tab.input)
      const recordedPath = recordedByTab.get(tab)
      if (
        filePaths.some(isExcluded) ||
        (recordedPath !== undefined && isExcluded(recordedPath))
      ) {
        continue
      }

      // 先按重置范围筛出候选 tab（作用域不变，仍只处理受本次重置影响的 tab）。
      const inResetScope =
        filePaths.some((filePath) =>
          targets.some((target) => directoryIsWithin(target, filePath)),
        ) ||
        (isPreview && (!hasRecords || recordedByTab.has(tab)))

      if (!inResetScope) {
        continue
      }

      matchingTabs.push({ tab, group })
    }
  }
  return matchingTabs
}
