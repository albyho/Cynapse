import type { ApplyOutcome, EditorFacade, PreparedTarget } from './applyModel'
import type { TypewriterChunk } from '@cynapse/adapter-kit'

/** One chunked editing transaction: prepare, apply, save, restore. */
export interface ChunkedEditPlan {
  target: PreparedTarget
  chunks: TypewriterChunk[]
  speedMs: number
  /** Runs once before the first chunk; false means no edit was made. */
  prepare?: () => Promise<boolean>
  applyChunk: (
    chunk: TypewriterChunk,
    index: number,
    isLast: boolean,
  ) => Promise<void>
  /** Restores the pre-Action document after at least one chunk was applied. */
  restore: () => Promise<void>
  /** Override the default delay after a non-final chunk. */
  delayAfter?: (
    chunk: TypewriterChunk,
    index: number,
    isLast: boolean,
  ) => number
}

/**
 * Shared transaction for typewriter editing: abort checks, undo grouping,
 * delays, save, and restore happen in one place. Each edit mode supplies only
 * its chunks and how to apply and restore them.
 */
export async function runChunkedEdit(
  plan: ChunkedEditPlan,
  facade: EditorFacade,
  signal?: AbortSignal,
): Promise<ApplyOutcome> {
  if (signal?.aborted) {
    return 'cancelled'
  }
  if (plan.prepare && !(await plan.prepare())) {
    return 'cancelled'
  }

  let applied = 0
  for (const [index, chunk] of plan.chunks.entries()) {
    if (signal?.aborted) {
      return restoreOrCancel(plan, applied)
    }
    const isLast = index === plan.chunks.length - 1
    await plan.applyChunk(chunk, index, isLast)
    applied += 1

    if (!isLast) {
      const delay = plan.delayAfter
        ? plan.delayAfter(chunk, index, isLast)
        : plan.speedMs * chunk.units
      if (delay > 0) {
        await facade.delay(delay)
      }
    }
  }

  if (signal?.aborted) {
    return restoreOrCancel(plan, applied)
  }
  await facade.save(plan.target)
  if (signal?.aborted) {
    return restoreOrCancel(plan, applied)
  }
  return 'done'
}

async function restoreOrCancel(
  plan: ChunkedEditPlan,
  applied: number,
): Promise<ApplyOutcome> {
  if (applied === 0) {
    return 'cancelled'
  }
  await plan.restore()
  return 'cancelled'
}
