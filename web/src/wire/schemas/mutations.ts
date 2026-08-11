/**
 * Mutation response schemas — v2.1 §7 batch 1 (2026-07-08).
 *
 * Every mutation on a thread lands here. Backend split into two
 * shapes:
 *
 * - **204 No Content** — the vast majority (star / unstar / pin /
 *   unpin / archive / unarchive / read / unread / snooze / delete).
 *   Handled by `wireFetch(..., { allowEmpty: true })` returning
 *   `undefined`. The parse target is `emptyResponseSchema` which
 *   accepts `undefined`.
 *
 * - **200 with JSON envelope** — batch and mark-all-read. Those get
 *   full schemas below.
 */

import { z } from 'zod'

/**
 * 204 No Content response schema — parse target is `undefined`.
 * `wireFetch` short-circuits before schema.safeParse when `res.status
 * === 204 && allowEmpty === true`, so the schema is just the
 * placeholder for `T` in `wireFetch<T>()`.
 */
export const emptyResponseSchema = z.undefined()

/**
 * `POST /api/conversations/batch` — bulk mutation across N threads.
 */
export const batchMutationResponseSchema = z.object({
  failed: z.number().int().min(0),
  /**
   * Which ones did not go through.
   *
   * The route has always known — it is a per-thread loop — and used to
   * report only a count, so a caller that removed fifty rows
   * optimistically learned that three failed and not which three, and
   * could roll back everything or nothing. Added 2026-08-12; omitted
   * from the JSON when empty, hence the default.
   */
  failed_thread_ids: z.array(z.string()).default([]),
  message: z.string().optional(),
  processed: z.number().int().min(0),
  success: z.boolean(),
})

export type WireBatchMutationResponse = z.infer<typeof batchMutationResponseSchema>

/**
 * `POST /api/conversations/mark-all-read` — flip every unread thread.
 * Backend returns the count flipped.
 */
export const markAllReadResponseSchema = z.object({
  flipped: z.number().int().min(0).default(0),
  success: z.boolean(),
})

export type WireMarkAllReadResponse = z.infer<typeof markAllReadResponseSchema>
