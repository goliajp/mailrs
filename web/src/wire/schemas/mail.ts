/**
 * Mail write / drafts / reactions / feedback schemas — v2.1 §9
 * batch 3 (2026-07-08), audit-repaired 2026-07-08 (§10-audit).
 *
 * Every list/read shape here is verified against the Rust handler
 * signature in `crates/webapi/src/handlers/{prefs,mail,complete}.rs`
 * and the `DraftWire` / `SaveDraftResponse` / `ReactionsResponse`
 * structs in `crates/core-api/src/method/admin.rs`.
 */

import { z } from 'zod'

/** id fields on the wire come as either i64 (drafts) or hex string
 * (message_id). Callers that need to round-trip a value use it as
 * an opaque token. Normalising to string simplifies downstream. */
const wireIdSchema = z.union([z.string(), z.number()]).transform((v) => String(v))

const wireTimestampSchema = z.union([z.string(), z.number()]).nullish()

// ── /mail/send ────────────────────────────────────────────────────

export const sendResultSchema = z
  .object({
    message: z.string().optional(),
    message_id: z.string().optional(),
    success: z.boolean().default(true),
  })
  .passthrough()

export type WireSendResult = z.infer<typeof sendResultSchema>

export const inlineUploadResultSchema = z
  .object({
    message: z.string().optional(),
    success: z.boolean().default(false),
    url: z.string().optional(),
  })
  .passthrough()

export type WireInlineUploadResult = z.infer<typeof inlineUploadResultSchema>

export const snoozeResultSchema = z
  .object({
    message: z.string().optional(),
    success: z.boolean().default(true),
  })
  .passthrough()

export type WireSnoozeResult = z.infer<typeof snoozeResultSchema>

// ── /mail/drafts ──────────────────────────────────────────────────
//
// Backend `DraftWire`:
//   {id: i64, to: String, cc: String, bcc: String, subject: String,
//    body: String, reply_to_thread_id: Option<String>,
//    created_at: i64, updated_at: i64}
// (see `crates/core-api/src/method/admin.rs`). Frontend originally
// typed these as `to_addresses` / `cc_addresses` / `bcc_addresses`
// with string timestamps — every draft list Zod-parsed to an empty
// object under Zod strip (default '' for the wrong-name fields,
// validation error for the wrong-type timestamps).

export const draftSchema = z
  .object({
    bcc: z.string().default(''),
    body: z.string().default(''),
    cc: z.string().default(''),
    created_at: wireTimestampSchema,
    id: wireIdSchema,
    reply_to_thread_id: z.string().nullable().default(null),
    subject: z.string().default(''),
    to: z.string().default(''),
    updated_at: wireTimestampSchema,
  })
  .passthrough()

export type WireDraft = z.infer<typeof draftSchema>

export const draftListSchema = z.union([
  z.object({ items: z.array(draftSchema) }),
  z.array(draftSchema).transform((items) => ({ items })),
])

/**
 * Backend `SaveDraftResponse` = `{id: i64}` — no envelope. Frontend
 * schema previously required `success: bool` which the handler
 * doesn't emit, so every draft save silently failed validation.
 */
export const saveDraftResultSchema = z
  .object({
    id: wireIdSchema,
    message: z.string().optional(),
    success: z.boolean().default(true),
  })
  .passthrough()

export type WireSaveDraftResult = z.infer<typeof saveDraftResultSchema>

export const deleteDraftResultSchema = z
  .object({
    message: z.string().optional(),
    success: z.boolean().default(true),
  })
  .passthrough()

export type WireDeleteDraftResult = z.infer<typeof deleteDraftResultSchema>

// ── /mail/feedback ────────────────────────────────────────────────

export const feedbackResultSchema = z
  .object({
    message: z.string().optional(),
    success: z.boolean().default(true),
  })
  .passthrough()

export type WireFeedbackResult = z.infer<typeof feedbackResultSchema>

// ── conversation reactions ────────────────────────────────────────
//
// Backend `ReactionsResponse` = `{reactions: Vec<ReactionAggregateRow>}`
// where each row is `{message_uid: i64, emoji, count: i64, me: bool}`.
// This is a FLAT list — the earlier `threadReactionsSchema` used
// a per-uid map (`z.record`) which never matched, so every
// `wireGetThreadReactions` failed validation.

export const reactionSummarySchema = z
  .object({
    count: z.number().int().min(0).default(0),
    emoji: z.string(),
    me: z.boolean().default(false),
    message_uid: z.number().int().optional(),
  })
  .passthrough()

export type WireReactionSummary = z.infer<typeof reactionSummarySchema>

export const reactionsListSchema = z
  .object({
    reactions: z.array(reactionSummarySchema).default([]),
  })
  .passthrough()

/** Same wire shape as `reactionsListSchema` — the "thread"-scoped
 *  endpoint returns a flat list including `message_uid` per row.
 *  Callers group by `message_uid` client-side if needed. */
export const threadReactionsSchema = reactionsListSchema
