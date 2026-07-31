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
//
// Backend: crates/webapi/src/handlers/complete.rs:1308 — list_agent_keys /
// :1328 create_agent_key. Row shape (verified against prod network kevy
// 2026-07-18, re-verified against the handler 2026-07-29): {id: i64, name:
// String, scopes: Vec<String>, created_at: i64, prefix: String}. `scopes`
// is an ARRAY — declaring it z.string() failed every parse and blanked the
// list ("No API keys" while the key existed).
//
// There is NO `expires_at`: the record written at complete.rs:1335 has no
// expiry field and `create_agent_key` ignores any expiry in the request.
// The transform used to emit a permanently-null `expires_at`, which the UI
// rendered as if expiry were a supported feature — dropped 2026-07-29.

const rawAgentKeySchema = z
  .object({
    created_at: wireTimestampSchema,
    id: wireIdSchema,
    name: z.string().default(''),
    prefix: z.string().default(''),
    scopes: z.array(z.string()).default([]),
  })
  .passthrough()

export const agentKeySchema = rawAgentKeySchema.transform((v) => ({
  created_at: v.created_at != null ? String(v.created_at) : '',
  id: v.id,
  name: v.name,
  prefix: v.prefix,
  scopes: v.scopes,
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
// Backend: crates/webapi/src/handlers/calendar.rs — `list_feeds` returns
// `{items: Vec<FeedView>}`, built from
// `core_sidestate::families::calendar_feeds::FeedRow`.
//
// `last_synced_at` and `last_error` used to be hardcoded `null` here,
// because nothing wrote them: a feed subscription stored a row and no
// worker read it, so the page could only ever say "never synced". Both are
// now real, and a feed that has been failing shows why. `has_basic_auth`
// says whether credentials are stored without returning the password.
//
// The UI's field is `refresh_interval_secs`; the backend's is
// `sync_interval_secs`. Renamed in the transform.

export const calendarFeedSchema = z
  .object({
    color: z.string().nullish(),
    created_at: wireTimestampSchema,
    has_basic_auth: z.boolean().default(false),
    id: wireIdSchema,
    last_error: z.string().nullish(),
    last_event_count: z.number().int().default(0),
    last_synced_at: z.number().int().default(0),
    name: z.string().default(''),
    sync_interval_secs: z.number().int().min(0).default(3600),
    url: z.string().default(''),
  })
  .passthrough()
  .transform((v) => ({
    enabled: true,
    hasBasicAuth: v.has_basic_auth,
    id: Number(v.id),
    last_error: v.last_error ?? null,
    lastEventCount: v.last_event_count,
    // Epoch seconds; zero means never, which is not the same as a sync at
    // the epoch and must not render as 1970.
    last_synced_at: v.last_synced_at === 0 ? null : v.last_synced_at,
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
