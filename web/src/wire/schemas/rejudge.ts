/**
 * The batches re-judging took back into Review, and the super
 * administrator's decisions on them.
 *
 * Backend: crates/webapi/src/handlers/rejudge.rs — `batches`, `confirm`,
 * `discard`, `release`; the list items are the core's
 * `mailrs_core_api::method::admin::RejudgeBatchWire` as it came.
 */

import { z } from '@/lib/zod'

export const wireRecallBatchSchema = z.object({
  /** When the first conversation was taken back, epoch seconds. */
  at: z.number(),
  /** Conversations the batch took back. */
  conversations: z.number(),
  /** When a waiting batch was discarded; 0 while it has not been. */
  discarded_at: z.number(),
  /** The observation whose round took them, `r<n>`. */
  observation: z.string(),
  /** The round would hold more than one observation may on its own and waits for a decision. */
  pending: z.boolean(),
  /** When the batch was put back; 0 while it has not been. */
  released_at: z.number(),
  /** The fraud rules it holds them for, by id. */
  rules: z.array(z.string()),
  /** Conversations the round found to hold when it was judged. */
  would_hold: z.number(),
})

export type WireRecallBatch = z.infer<typeof wireRecallBatchSchema>

export const wireRecallBatchesResponseSchema = z.object({
  items: z.array(wireRecallBatchSchema),
})

export const wireRecallBatchConfirmedSchema = z.object({ held: z.number() })
export const wireRecallBatchDiscardedSchema = z.object({ would_hold: z.number() })
export const wireRecallBatchReleasedSchema = z.object({ released: z.number() })
