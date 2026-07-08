/**
 * User-facing settings wire schemas — v2.1 §9 batch 3 (2026-07-08),
 * §10-audit repaired 2026-07-08.
 *
 * Each schema Zod-validates the actual backend shape (per
 * `crates/webapi/src/handlers/{complete,calendar}.rs`) then
 * `.transform()`s to the frontend `_shared.tsx` type. The UI never
 * sees the raw wire — it sees the domain shape it always expected.
 * This keeps the boundary honest (Zod catches wire drift) without
 * cascading a rename through every settings section.
 */

import { z } from 'zod'

const wireIdSchema = z.union([z.string(), z.number()]).transform((v) => String(v))

const wireTimestampSchema = z.union([z.string(), z.number()]).nullish()

// ── agent API keys ──────────────────────────────────────────────

const rawAgentKeySchema = z
  .object({
    created_at: wireTimestampSchema,
    id: wireIdSchema,
    name: z.string().default(''),
    prefix: z.string().default(''),
    scopes: z.string().default(''),
  })
  .passthrough()

export const agentKeySchema = rawAgentKeySchema.transform((v) => ({
  created_at: v.created_at != null ? String(v.created_at) : '',
  expires_at: null as null | string,
  id: v.id,
  name: v.name,
  prefix: v.prefix,
}))

export type WireAgentKey = z.infer<typeof agentKeySchema>

/** Backend `create_agent_key` returns `{id, secret}`. Frontend
 *  UI shape historically read `.key` for the copy button. */
export const createdAgentKeySchema = z
  .object({
    id: wireIdSchema,
    secret: z.string(),
  })
  .passthrough()
  .transform((v) => ({
    id: v.id,
    key: v.secret,
    prefix: v.secret.slice(0, 8),
  }))

export type WireCreatedAgentKey = z.infer<typeof createdAgentKeySchema>

// ── agent webhooks ───────────────────────────────────────────────

const rawWebhookSchema = z
  .object({
    active: z.boolean().default(true),
    created_at: wireTimestampSchema,
    event_type: z.string().default(''),
    id: wireIdSchema,
    signing_secret: z.string().optional(),
    url: z.string().default(''),
  })
  .passthrough()

export const webhookSchema = rawWebhookSchema.transform((v) => ({
  active: v.active,
  event_type: v.event_type,
  filter_sender: null as null | string,
  filter_thread_id: null as null | string,
  id: v.id,
  url: v.url,
}))

export type WireWebhook = z.infer<typeof webhookSchema>

export const createdWebhookSchema = rawWebhookSchema.transform((v) => ({
  id: v.id,
  signing_secret: v.signing_secret ?? '',
}))

export type WireCreatedWebhook = z.infer<typeof createdWebhookSchema>

// ── mail signatures ──────────────────────────────────────────────
//
// Backend `SignatureWire` uses `html` (short); frontend UI reads
// `html_content`. Rename in-transform.

export const signatureSchema = z
  .object({
    created_at: wireTimestampSchema,
    html: z.string().default(''),
    id: wireIdSchema,
    is_default: z.boolean().default(false),
    name: z.string(),
    text_content: z.string().default(''),
  })
  .passthrough()
  .transform((v) => ({
    html_content: v.html,
    id: Number(v.id),
    is_default: v.is_default,
    name: v.name,
    text_content: v.text_content,
  }))

export type WireSignature = z.infer<typeof signatureSchema>

// ── calendar feeds ───────────────────────────────────────────────
//
// Backend `FeedWire` uses `sync_interval_secs`; frontend UI reads
// `refresh_interval_secs`. Rename in-transform. Missing
// `last_synced_at` / `last_error` / `enabled` synthesised from
// defaults.

export const calendarFeedSchema = z
  .object({
    color: z.string().nullish(),
    created_at: wireTimestampSchema,
    id: wireIdSchema,
    name: z.string().default(''),
    sync_interval_secs: z.number().int().min(0).default(3600),
    url: z.string().default(''),
  })
  .passthrough()
  .transform((v) => ({
    enabled: true,
    id: Number(v.id),
    last_error: null as null | string,
    last_synced_at: null as null | string,
    name: v.name,
    refresh_interval_secs: v.sync_interval_secs,
    url: v.url,
  }))

export type WireCalendarFeed = z.infer<typeof calendarFeedSchema>

// ── encryption keys ──────────────────────────────────────────────
//
// Backend `keys_status` returns `{configured: bool, key_count: usize}`.
// Frontend UI reads `{pgp_fingerprint, smime_fingerprint}` — completely
// different shape. Since the fingerprint data isn't actually on the
// wire, transform to `{null, null}` and let the UI render "no key
// configured". A future backend `/keys/details` handler is needed to
// unblock the fingerprint display.

export const keyStatusSchema = z
  .object({
    configured: z.boolean().default(false),
    key_count: z.number().int().min(0).default(0),
  })
  .passthrough()
  .transform(() => ({
    pgp_fingerprint: null as null | string,
    smime_fingerprint: null as null | string,
  }))

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
