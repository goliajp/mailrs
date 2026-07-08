/**
 * User-facing settings wire schemas — v2.1 §9 batch 3 (2026-07-08).
 *
 * Covers the five settings sections:
 *  - agent API keys        `/api/agent/keys`
 *  - agent webhooks        `/api/agent/webhooks`
 *  - mail signatures       `/api/mail/signatures`
 *  - calendar feeds        `/api/calendar/feeds`
 *  - encryption keys       `/api/mail/keys/status`
 *
 * Same convention as `wire/schemas/conversation.ts`:
 *   - schema fields align with the Rust wire structs — not the
 *     frontend `_shared.tsx` types (see §8 for what happens when
 *     those drift).
 *   - default values keep the schema forgiving of missing / null
 *     fields; validate-tight when adding a new endpoint.
 */

import { z } from 'zod'

// ── agent API keys ──────────────────────────────────────────────

export const agentKeySchema = z.object({
  created_at: z.string(),
  expires_at: z.string().nullable().default(null),
  id: z.string(),
  name: z.string(),
  prefix: z.string(),
})

export type WireAgentKey = z.infer<typeof agentKeySchema>

export const createdAgentKeySchema = z.object({
  id: z.string(),
  key: z.string(),
  prefix: z.string(),
})

export type WireCreatedAgentKey = z.infer<typeof createdAgentKeySchema>

// ── agent webhooks ───────────────────────────────────────────────

export const webhookSchema = z.object({
  active: z.boolean().default(true),
  event_type: z.string(),
  filter_sender: z.string().nullable().default(null),
  filter_thread_id: z.string().nullable().default(null),
  id: z.string(),
  url: z.string(),
})

export type WireWebhook = z.infer<typeof webhookSchema>

export const createdWebhookSchema = z.object({
  id: z.string(),
  signing_secret: z.string(),
})

export type WireCreatedWebhook = z.infer<typeof createdWebhookSchema>

// ── mail signatures ──────────────────────────────────────────────

export const signatureSchema = z.object({
  html_content: z.string().default(''),
  id: z.number().int(),
  is_default: z.boolean().default(false),
  name: z.string(),
  text_content: z.string().default(''),
})

export type WireSignature = z.infer<typeof signatureSchema>

// ── calendar feeds ───────────────────────────────────────────────

export const calendarFeedSchema = z.object({
  enabled: z.boolean().default(true),
  id: z.number().int(),
  last_error: z.string().nullable().default(null),
  last_synced_at: z.string().nullable().default(null),
  name: z.string(),
  refresh_interval_secs: z.number().int().min(0).default(3600),
  url: z.string(),
})

export type WireCalendarFeed = z.infer<typeof calendarFeedSchema>

// ── encryption keys ──────────────────────────────────────────────

export const keyStatusSchema = z.object({
  pgp_fingerprint: z.string().nullable().default(null),
  smime_fingerprint: z.string().nullable().default(null),
})

export type WireKeyStatus = z.infer<typeof keyStatusSchema>

// ── list wrappers (enveloped `{items: [...]}` or bare array) ─────

export const agentKeyListSchema = z.union([
  z.object({ items: z.array(agentKeySchema) }),
  z.array(agentKeySchema).transform((items) => ({ items })),
])

export const webhookListSchema = z.union([
  z.object({ items: z.array(webhookSchema) }),
  z.array(webhookSchema).transform((items) => ({ items })),
])

export const signatureListSchema = z.union([
  z.object({ items: z.array(signatureSchema) }),
  z.array(signatureSchema).transform((items) => ({ items })),
])

export const calendarFeedListSchema = z.union([
  z.object({ items: z.array(calendarFeedSchema) }),
  z.array(calendarFeedSchema).transform((items) => ({ items })),
])
