/**
 * Mail write / drafts / reactions / feedback schemas — v2.1 §9
 * batch 3 (2026-07-08).
 */

import { z } from 'zod'

// ── /mail/send ────────────────────────────────────────────────────

export const sendResultSchema = z.object({
  message: z.string().optional(),
  message_id: z.string().optional(),
  success: z.boolean(),
})

export type WireSendResult = z.infer<typeof sendResultSchema>

export const inlineUploadResultSchema = z.object({
  message: z.string().optional(),
  success: z.boolean().default(false),
  url: z.string().optional(),
})

export type WireInlineUploadResult = z.infer<typeof inlineUploadResultSchema>

export const snoozeResultSchema = z.object({
  message: z.string().optional(),
  success: z.boolean().default(true),
})

export type WireSnoozeResult = z.infer<typeof snoozeResultSchema>

// ── /mail/drafts ──────────────────────────────────────────────────

export const draftSchema = z.object({
  bcc_addresses: z.string().default(''),
  body: z.string().default(''),
  cc_addresses: z.string().default(''),
  created_at: z.string(),
  id: z.number().int(),
  reply_to_thread_id: z.string().nullable().default(null),
  subject: z.string().default(''),
  to_addresses: z.string().default(''),
  updated_at: z.string(),
})

export type WireDraft = z.infer<typeof draftSchema>

export const draftListSchema = z.union([
  z.object({ items: z.array(draftSchema) }),
  z.array(draftSchema).transform((items) => ({ items })),
])

export const saveDraftResultSchema = z.object({
  id: z.number().int().optional(),
  message: z.string().optional(),
  success: z.boolean(),
})

export type WireSaveDraftResult = z.infer<typeof saveDraftResultSchema>

export const deleteDraftResultSchema = z.object({
  message: z.string().optional(),
  success: z.boolean(),
})

export type WireDeleteDraftResult = z.infer<typeof deleteDraftResultSchema>

// ── /mail/feedback ────────────────────────────────────────────────

export const feedbackResultSchema = z.object({
  message: z.string().optional(),
  success: z.boolean(),
})

export type WireFeedbackResult = z.infer<typeof feedbackResultSchema>

// ── conversation reactions ────────────────────────────────────────
//
// Backend (see `crates/webapi/src/handlers/mail.rs::get_thread_reactions`
// and the ReactionAggregateRow struct in
// `crates/core-api/src/method/admin.rs`) aggregates on read to
// `{emoji, count, me}` — no per-user list on the wire, `me` is the
// current-viewer's-vote boolean.

export const reactionSummarySchema = z.object({
  count: z.number().int().min(0).default(0),
  emoji: z.string(),
  me: z.boolean().default(false),
})

export type WireReactionSummary = z.infer<typeof reactionSummarySchema>

export const reactionsListSchema = z.object({
  reactions: z.array(reactionSummarySchema).default([]),
})

export const threadReactionsSchema = z.object({
  reactions: z.record(z.string(), z.array(reactionSummarySchema)).default({}),
})
