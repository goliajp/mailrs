/**
 * Settings CRUD endpoints — v2.1 §9 batch 3 (2026-07-08).
 *
 * User-facing settings sections. Every hook that used to hit
 * `postJson` / `fetchList` / `deleteJson` / `putJson` against
 * `/api/agent/*`, `/api/mail/signatures`, `/api/calendar/feeds`,
 * `/api/mail/keys/*` now routes through here.
 */

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import {
  agentKeyListSchema,
  calendarFeedListSchema,
  createdAgentKeySchema,
  createdWebhookSchema,
  keyStatusSchema,
  senderListSchema,
  signatureListSchema,
  webhookListSchema,
  type WireAgentKey,
  type WireCalendarFeed,
  type WireCreatedAgentKey,
  type WireCreatedWebhook,
  type WireKeyStatus,
  type WireSignature,
  type WireWebhook,
} from '../schemas/settings'

// ── agent API keys ──────────────────────────────────────────────

/**
 * Backend: `crates/webapi/src/handlers/complete.rs:1320` —
 * `CreateAgentKeyRequest { name, scopes }`. It reads nothing else; the
 * `expires_in_days` this used to send was dropped on the floor by serde
 * while the UI reported the key as expiring.
 */
export async function wireCreateAgentKey(payload: {
  name: string
  scopes?: string[]
}): Promise<WireCreatedAgentKey> {
  return wireFetch(createdAgentKeySchema, {
    body: payload,
    method: 'POST',
    path: '/agent/keys',
  })
}

/**
 * Backend: crates/webapi/src/handlers/calendar.rs — `create_feed` takes
 * `{name, url, color?, sync_interval_secs?}`.
 *
 * The interval is `sync_interval_secs` on the wire; the response schema
 * already renames it to `refresh_interval_secs` for the UI, and the
 * request now renames the other way instead of sending the UI's name and
 * having it silently defaulted.
 *
 * `basic_auth_user` / `basic_auth_pass` / `enabled` are gone. No handler
 * named them, so they were dropped on arrival, and the prod lane has no
 * feed fetcher to use them (`spawn_feed_worker` is monolith-only).
 */
/**
 * Backend: crates/webapi/src/handlers/calendar.rs — `create_feed`, taking
 * `CreateFeedRequest`.
 *
 * The basic-auth pair is sent again now that a fetcher consumes it; the
 * inputs were removed on 2026-07-30 precisely because nothing did.
 */
export async function wireCreateCalendarFeed(payload: {
  basicAuthPass?: string
  basicAuthUser?: string
  color?: null | string
  name: string
  refreshIntervalSecs?: number
  url: string
}): Promise<void> {
  const body: Record<string, unknown> = { name: payload.name, url: payload.url }
  if (payload.color) body['color'] = payload.color
  if (payload.refreshIntervalSecs !== undefined) {
    body['sync_interval_secs'] = payload.refreshIntervalSecs
  }
  if (payload.basicAuthUser) body['basic_auth_user'] = payload.basicAuthUser
  if (payload.basicAuthPass) body['basic_auth_pass'] = payload.basicAuthPass
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body,
    method: 'POST',
    path: '/calendar/feeds',
  })
}

/**
 * `POST /mail/signatures` — create OR update. Backend treats
 * `id` as the discriminator: present → update, absent → create.
 *
 * Backend: `SaveSignatureRequest` in crates/core-api/src/method/admin.rs
 * names the body `html`. The UI calls it `html_content`, and the response
 * schema already renames one way; the request did not, and because `html`
 * carries `#[serde(default)]` the mismatch raised no error — **every
 * signature was saved with an empty HTML body**. That is the
 * "signature preview is always blank" symptom from the 2026-07-08 wire
 * audit, which fixed the response half only.
 */
export async function wireCreateSignature(payload: {
  html_content: string
  id?: number
  is_default?: boolean
  name: string
  text_content: string
}): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: {
      html: payload.html_content,
      id: payload.id,
      is_default: payload.is_default ?? false,
      name: payload.name,
      text_content: payload.text_content,
    },
    method: 'POST',
    path: '/mail/signatures',
  })
}

// ── agent webhooks ───────────────────────────────────────────────

export async function wireCreateWebhook(payload: {
  event_type: string
  filter_sender?: null | string
  filter_thread_id?: null | string
  url: string
}): Promise<WireCreatedWebhook> {
  return wireFetch(createdWebhookSchema, {
    body: payload,
    method: 'POST',
    path: '/agent/webhooks',
  })
}

export async function wireDeleteAgentKey(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/agent/keys/${encodeURIComponent(id)}`,
  })
}

export async function wireDeleteCalendarFeed(id: number): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/calendar/feeds/${id}`,
  })
}

// ── mail signatures ──────────────────────────────────────────────

export async function wireDeleteSignature(id: number): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/mail/signatures/${id}`,
  })
}

export async function wireDeleteWebhook(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/agent/webhooks/${encodeURIComponent(id)}`,
  })
}

export async function wireListAgentKeys(): Promise<readonly WireAgentKey[]> {
  const raw = await wireFetch(agentKeyListSchema, { path: '/agent/keys' })
  return raw.items
}

// ── calendar feeds ───────────────────────────────────────────────

export async function wireListCalendarFeeds(): Promise<readonly WireCalendarFeed[]> {
  const raw = await wireFetch(calendarFeedListSchema, { path: '/calendar/feeds' })
  return raw.items
}

export async function wireListSignatures(): Promise<readonly WireSignature[]> {
  const raw = await wireFetch(signatureListSchema, { path: '/mail/signatures' })
  return raw.items
}

export async function wireListWebhooks(): Promise<readonly WireWebhook[]> {
  const raw = await wireFetch(webhookListSchema, { path: '/agent/webhooks' })
  return raw.items
}

// ── encryption keys ──────────────────────────────────────────────

export const wireGetKeyStatus = (): Promise<WireKeyStatus> =>
  wireFetch(keyStatusSchema, { path: '/mail/keys/status' })

export type SenderListName = 'blacklist' | 'whitelist'

export async function wireAddSender(list: SenderListName, address: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { address },
    method: 'POST',
    path: list === 'whitelist' ? '/spam/whitelist' : '/spam/blacklist',
  })
}

// ── sender lists ─────────────────────────────────────────────────
//
// `spam:{user}:whitelist` is live: marking a conversation *not junk*
// adds its sender, and the inbound pipeline reads the set on every
// delivery. These four routes have existed since before either client
// did and neither ever called them — so the list could only ever
// grow, and nobody could see what was on it.
//
// Backend: `crates/webapi/src/handlers/spam_lists.rs` —
// `{"entries": [...]}` on GET, `AddRequest { address }` on POST.

export async function wireDeleteKey(type: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/mail/keys/${encodeURIComponent(type)}`,
  })
}

export async function wireListSenders(list: SenderListName): Promise<readonly string[]> {
  // Spelled out rather than interpolated. `/spam/${list}` is the same
  // request and is invisible to `check-dead-routes.sh`, which can only
  // see the literals a file holds — a route that *is* called looking
  // like one nobody calls is exactly the report that gate exists to
  // give.
  const path = list === 'whitelist' ? '/spam/whitelist' : '/spam/blacklist'
  const raw = await wireFetch(senderListSchema, { path })
  return raw.entries
}

export async function wireRemoveSender(list: SenderListName, address: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path:
      list === 'whitelist'
        ? `/spam/whitelist/${encodeURIComponent(address)}`
        : `/spam/blacklist/${encodeURIComponent(address)}`,
  })
}

/**
 * Backend: `SetKeyRequest` in crates/webapi/src/handlers/keys.rs (and the
 * identical struct in the monolith's mail/keys.rs) requires `public_key`.
 * This sent `{content}`, so every upload failed deserialization with a
 * missing-field 422 and no key was ever stored.
 *
 * `fingerprint` is not sent. It is `#[serde(default)]` on both backends and
 * neither derives it — it is a label, and claiming one we have not computed
 * would store a value that may not describe the key.
 */
export async function wireUploadKey(type: string, content: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { public_key: content },
    method: 'PUT',
    path: `/mail/keys/${encodeURIComponent(type)}`,
  })
}
