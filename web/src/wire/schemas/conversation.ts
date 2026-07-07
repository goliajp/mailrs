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

export const wireThreadSummarySchema = z.object({
  category: z.string().default('inbox'),
  flagged: z.boolean().default(false),
  folder: z.string().nullish(),
  importance_level: z.string().nullish(),
  last_date: z.number().default(0),
  message_count: z.number().int().min(0).default(0),
  participants: z.array(wireParticipantSchema).default([]),
  pinned: z.boolean().default(false),
  requires_action: z.boolean().default(false),
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
  filename: z.string().default(''),
  index: z.number().int().min(0).default(0),
  size: z.number().int().min(0).default(0),
})

export const wireMessageSchema = z.object({
  attachments: z.array(wireAttachmentSchema).default([]),
  bcc: z.array(z.string()).default([]),
  cc: z.array(z.string()).default([]),
  flags: z.number().int().default(0),
  html_body: z.string().nullish(),
  internal_date: z.number().default(0),
  message_id: z.string().default(''),
  recipients: z.array(z.string()).default([]),
  sender: z.string().default(''),
  subject: z.string().default(''),
  text_body: z.string().default(''),
  thread_id: z.string(),
  uid: z.number().int().min(0),
})

export type WireMessage = z.infer<typeof wireMessageSchema>

export const wireThreadDetailResponseSchema = z.union([
  z.object({
    messages: z.array(wireMessageSchema),
    thread_id: z.string(),
  }),
  z.object({
    items: z.array(wireMessageSchema),
  }),
])

export type WireThreadDetailResponse = z.infer<typeof wireThreadDetailResponseSchema>
