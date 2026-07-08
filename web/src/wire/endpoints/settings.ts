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

export async function wireCreateAgentKey(payload: {
  expires_in_days?: null | number
  name: string
}): Promise<WireCreatedAgentKey> {
  return wireFetch(createdAgentKeySchema, {
    body: payload,
    method: 'POST',
    path: '/agent/keys',
  })
}

export async function wireCreateCalendarFeed(payload: {
  basic_auth_pass?: null | string
  basic_auth_user?: null | string
  enabled?: boolean
  name: string
  refresh_interval_secs?: number
  url: string
}): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: payload,
    method: 'POST',
    path: '/calendar/feeds',
  })
}

/**
 * `POST /mail/signatures` — create OR update. Backend treats
 * `id` as the discriminator: present → update, absent → create.
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
    body: payload,
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

export async function wireDeleteKey(type: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/mail/keys/${encodeURIComponent(type)}`,
  })
}

export async function wireUploadKey(type: string, content: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { content },
    method: 'PUT',
    path: `/mail/keys/${encodeURIComponent(type)}`,
  })
}
