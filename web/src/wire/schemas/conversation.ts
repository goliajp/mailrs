/**
 * Zod schemas for the conversation family endpoints — layer 2.
 *
 * Every wire response is parsed through one of these schemas; the
 * parsed value has a shape safe to feed into the domain adapters.
 * When the backend changes a field, the schema fails loudly and the
 * openapi-diff CI gate catches the drift before it reaches prod.
 */

import { z } from 'zod'

/**
 * A single participant from the wire.
 * The wire sometimes sends encoded-word MIME strings — the wire
 * layer decodes them when it produces the domain `Participant`.
 * Backend uses `first_from`, `senders_csv`, `to`, `cc` for message-
 * level shapes; on the list surface it collapses to `participants`.
 */
export const wireParticipantSchema = z.string()

/**
 * Complete shape of a `ConversationSummary` on the wire. Every field
 * the `use-mail-queries::useConversationsQuery` / `use-mail-events`
 * pipeline downstream reads must be listed here — Zod strips unknown
 * fields silently on parse, so a missing entry here silently drops
 * that field for every consumer.
 *
 * v2.1 Phase A §7 (2026-07-08): expanded to full ConversationSummary
 * coverage. Prior 14-field version dropped archived / importance_score
 * / received_count / sent_count — enough downstream
 * damage to make swapping in `wireFetch` unsafe. Regenerating this
 * from the Rust wire type is a §D task; for now grep
 * `types.ts::ConversationSummary` to sync.
 */
export const wireThreadSummarySchema = z.object({
  archived: z.boolean().default(false),
  category: z.string().default('inbox'),
  flagged: z.boolean().default(false),
  folder: z.string().nullish(),
  importance_level: z.string().default('normal'),
  importance_score: z.number().default(0),
  last_date: z.number().default(0),
  message_count: z.number().int().min(0).default(0),
  participants: z.array(wireParticipantSchema).default([]),
  pinned: z.boolean().default(false),
  received_count: z.number().int().min(0).default(0),
  requires_action: z.boolean().default(false),
  sent_count: z.number().int().min(0).default(0),
  snippet: z.string().default(''),
  snoozed_until: z.number().nullish(),
  subject: z.string().default(''),
  thread_id: z.string(),
  unread_count: z.number().int().min(0).default(0),
})

export type WireThreadSummary = z.infer<typeof wireThreadSummarySchema>

/**
 * Both shapes the list endpoint has been observed to return
 * historically. See `web/src/lib/api.ts::fetchList` for the class of
 * bugs this consolidates.
 */
export const wireThreadListResponseSchema = z.union([
  z.object({
    items: z.array(wireThreadSummarySchema),
    /**
     * Backend attaches counts for filter chips; take them if present
     * and ignore otherwise — the view-model derives its own.
     */
    stats: z.unknown().optional(),
  }),
  z.array(wireThreadSummarySchema).transform((items) => ({ items, stats: undefined })),
])

export type WireThreadListResponse = z.infer<typeof wireThreadListResponseSchema>

// ── message-level ───────────────────────────────────────────────────

export const wireAttachmentSchema = z.object({
  content_type: z.string().default('application/octet-stream'),
  // v2.5.0 Phase 5 (RFC-B §5) — MIME `Content-ID` header value with
  // angle brackets stripped. Present on `multipart/related` inline
  // images that the HTML body references via `<img src="cid:..">`.
  // Frontend HtmlFrame rewrites those cid: URIs so the browser can
  // fetch the referenced image from the attachment endpoint.
  content_id: z.string().nullish(),
  filename: z.string().default(''),
  index: z.number().int().min(0).default(0),
  size: z.number().int().min(0).default(0),
})

/**
 * Wire mirror of `ThreadMessage` in `types.ts`. As with
 * `wireThreadSummarySchema`, keep this in sync with the Rust
 * `ThreadMessageResponse` — Zod strips unknowns silently, so a
 * missing entry here silently drops the field for downstream.
 *
 * v2.1 Phase A §7 (2026-07-08): expanded from 13 to full 28-field
 * coverage. Prior version stripped every AI analysis field
 * (category, summary, people, dates, amounts, action_items,
 * importance_*, is_bulk_sender, has_tracking_pixel, sender_intent,
 * clean_text, new_content, structured_data, invite_method,
 * risk_score, risk_reason, ai_analyzed, action_deadline). Also
 * `recipients` and `cc` are strings on the wire (comma-separated),
 * not arrays — align with `types.ts`.
 *
 * Note the `id` field is **absent** — deleted from the wire in the
 * same commit that migrated the timeline to `key={msg.uid}` (see
 * `commit 67e79e64`).
 */
export const wireMessageSchema = z.object({
  action_deadline: z.string().nullish(),
  action_items: z.unknown().default([]),
  ai_analyzed: z.boolean().default(false),
  amounts: z.unknown().default([]),
  attachments: z.array(wireAttachmentSchema).default([]),
  bimi_logo_url: z.string().nullish(),
  category: z.string().default('inbox'),
  cc: z.string().nullish(),
  clean_text: z.string().nullish(),
  dates: z.unknown().default([]),
  flags: z.number().int().default(0),
  has_tracking_pixel: z.boolean().default(false),
  html_body: z.string().nullish(),
  importance_level: z.string().default('normal'),
  importance_score: z.number().default(0),
  internal_date: z.number().default(0),
  invite_method: z.string().nullish(),
  is_bulk_sender: z.boolean().default(false),
  message_id: z.string().default(''),
  new_content: z.string().nullish(),
  people: z.unknown().default([]),
  recipients: z.string().default(''),
  requires_action: z.boolean().default(false),
  risk_reason: z.string().default(''),
  risk_score: z.number().default(0),
  sender: z.string().default(''),
  sender_intent: z.string().default('inform'),
  structured_data: z.unknown().nullish(),
  subject: z.string().default(''),
  summary: z.string().default(''),
  text_body: z.string().nullable().default(''),
  uid: z.number().int().min(0),
})

export type WireMessage = z.infer<typeof wireMessageSchema>

/**
 * Backend `GET /api/conversations/{id}` returns a **bare** `Vec<
 * ThreadMessageResponse>` (see `crates/webapi/src/handlers/
 * conversations.rs::get_thread_messages` — `Json<Vec<_>>`). The
 * earlier union of `{messages, thread_id}` / `{items}` never
 * matched, so Zod threw `validation` and the thread view rendered
 * empty (user-reported 2026-07-08 "看不到邮件正文" regression).
 * Every observed shape now normalises to `{items: WireMessage[]}`.
 */
export const wireThreadDetailResponseSchema = z.union([
  z.array(wireMessageSchema).transform((items) => ({ items })),
  z
    .object({
      messages: z.array(wireMessageSchema),
      thread_id: z.string(),
    })
    .transform((v) => ({ items: v.messages })),
  z.object({
    items: z.array(wireMessageSchema),
  }),
])

export type WireThreadDetailResponse = z.infer<typeof wireThreadDetailResponseSchema>
