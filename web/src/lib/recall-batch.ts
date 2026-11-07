/**
 * What a recall batch's fields say about it. The server keeps no state
 * field: a waiting round is `pending`; deciding clears that, and a discard
 * also stamps `discarded_at`; a put-back stamps `released_at`. A batch that
 * is none of these holds its conversations, whether it was confirmed or
 * held on its own.
 */

import type { WireRecallBatch } from '@/wire/schemas/rejudge'

export type RecallBatchState = 'discarded' | 'held' | 'pending' | 'released'

/** A waiting round has held nothing yet: count what it would hold. */
export function recallBatchCount(b: WireRecallBatch): number {
  if (b.pending) return b.would_hold
  return b.conversations
}

export function recallBatchState(b: WireRecallBatch): RecallBatchState {
  if (b.pending) return 'pending'
  if (b.discarded_at > 0) return 'discarded'
  if (b.released_at > 0) return 'released'
  return 'held'
}
