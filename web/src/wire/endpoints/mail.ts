/**
 * Mail write endpoints — v2.1 §9 batch 3 (2026-07-08).
 *
 * `/mail/send` (JSON), `/mail/drafts` CRUD, `/mail/feedback`,
 * `/mail/pending/{id}` (undo send), reactions PUT / GET.
 *
 * `/mail/send-multipart` and `/mail/inline-upload` are FormData
 * bodies — deferred to a follow-up (`wireFetch` FormData support
 * is planned for §D).
 */

import { getToken } from '@/store/auth'

import { wireFetch } from '../client'
import {
  deleteDraftResultSchema,
  draftListSchema,
  feedbackResultSchema,
  inlineUploadResultSchema,
  reactionsListSchema,
  saveDraftResultSchema,
  sendResultSchema,
  sentMessagesSchema,
  snoozeResultSchema,
  threadReactionsSchema,
  unsubscribeResultSchema,
  type WireDeleteDraftResult,
  type WireDraft,
  type WireFeedbackResult,
  type WireInlineUploadResult,
  type WireReactionSummary,
  type WireSaveDraftResult,
  type WireSendResult,
  type WireSentMessage,
  type WireSnoozeResult,
} from '../schemas/mail'
import { emptyResponseSchema } from '../schemas/mutations'

// ── /mail/send ────────────────────────────────────────────────────

export const wireSendMailJson = (payload: Record<string, unknown>): Promise<WireSendResult> =>
  wireFetch(sendResultSchema, {
    body: payload,
    method: 'POST',
    path: '/mail/send',
  })

/**
 * Multipart send — attachments path. The browser derives the correct
 * multipart boundary from FormData, so we pass `bodyRaw` and let the
 * transport skip the JSON path.
 */
export const wireSendMailMultipart = (fd: FormData): Promise<WireSendResult> =>
  wireFetch(sendResultSchema, {
    bodyRaw: fd,
    method: 'POST',
    path: '/mail/send-multipart',
  })

// ── snooze / unsnooze conversation ────────────────────────────────

/**
 * Backend: fastcore `handlers::conversations::{snooze_thread,
 * unsnooze_thread}` both answer 204 with no body. Same 204-vs-object
 * mismatch that broke draft deletion (2026-07-19) — `allowEmpty` is
 * required, and the schema stays optional so a monolith-style
 * `{success, message}` envelope still parses.
 */
/**
 * Backend: `SnoozeBody { snoozed_until: i64 }` in
 * crates/webapi/src/handlers/conversations.rs — Unix epoch **seconds**.
 *
 * This sent `{until: <ISO string>}`, which is neither the field name nor
 * the type, so every snooze failed deserialization with a 422 and no
 * thread was ever snoozed on the fastcore lane. The name and the unit both
 * now match the handler, and the monolith's `SnoozeRequest` was changed to
 * agree rather than being left accepting a third form.
 *
 * Seconds, matching `scheduled_at`, so the API has one time format.
 */
export const wireSnoozeConversation = (
  threadId: string,
  snoozedUntil: number
): Promise<undefined | WireSnoozeResult> =>
  wireFetch(snoozeResultSchema.optional(), {
    allowEmpty: true,
    body: { snoozed_until: snoozedUntil },
    method: 'PUT',
    path: `/conversations/${encodeURIComponent(threadId)}/snooze`,
  })

export const wireUnsnoozeConversation = (threadId: string): Promise<undefined | WireSnoozeResult> =>
  wireFetch(snoozeResultSchema.optional(), {
    allowEmpty: true,
    method: 'DELETE',
    path: `/conversations/${encodeURIComponent(threadId)}/snooze`,
  })

// ── /mail/inline-upload ──────────────────────────────────────────

export const wireUploadInlineImage = (file: File): Promise<WireInlineUploadResult> => {
  const fd = new FormData()
  fd.append('image', file)
  return wireFetch(inlineUploadResultSchema, {
    bodyRaw: fd,
    method: 'POST',
    path: '/mail/inline-upload',
  })
}

// ── /mail/pending (undo send) ─────────────────────────────────────

/**
 * Backend: crates/webapi/src/handlers/prefs.rs — `save_draft`, taking
 * `mailrs_core_api::method::admin::SaveDraftRequest`.
 *
 * The payload used to be `Record<string, unknown>`, so a misspelled or
 * renamed field compiled and serde dropped it on arrival. The autosave that
 * runs every three seconds is this call; when it fails it fails identically
 * forever, and until 2026-07-31 it did so behind an empty catch.
 *
 * `id` absent allocates a draft, `id` present upserts that one — which is
 * what keeps a compose session updating a single draft instead of spawning
 * one per tick.
 */
export type WireSaveDraftRequest = {
  bcc?: string
  body?: string
  cc?: string
  id?: number
  reply_to_thread_id?: string
  subject?: string
  to?: string
}

// ── /mail/drafts ──────────────────────────────────────────────────

export async function wireDeletePendingSend(messageId: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/mail/pending/${encodeURIComponent(messageId)}`,
  })
}

export async function wireListDrafts(): Promise<readonly WireDraft[]> {
  const raw = await wireFetch(draftListSchema, { path: '/mail/drafts' })
  return raw.items
}

export function wireListSentMessages(): Promise<readonly WireSentMessage[]> {
  return wireFetch(sentMessagesSchema, { path: '/mail/sent' })
}

export const wireSaveDraft = (payload: WireSaveDraftRequest): Promise<WireSaveDraftResult> =>
  wireFetch(saveDraftResultSchema, {
    body: payload,
    method: 'POST',
    path: '/mail/drafts',
  })

/**
 * Backend: fastcore `handlers::prefs::delete_draft` answers 204 with no
 * body; the monolith's `web/mail/drafts.rs::delete_draft` answers 200
 * with `{success, message}`. Both mean "gone", so accept either —
 * `allowEmpty` short-circuits the 204 and the union tolerates the JSON.
 * Without `allowEmpty` the 204 was parsed as `undefined` against an
 * object schema and surfaced as "Could not delete draft" even though
 * the draft had in fact been deleted (2026-07-19).
 */
export const wireDeleteDraft = (id: number): Promise<undefined | WireDeleteDraftResult> =>
  wireFetch(deleteDraftResultSchema.optional(), {
    allowEmpty: true,
    method: 'DELETE',
    path: `/mail/drafts/${id}`,
  })

// ── /mail/feedback ────────────────────────────────────────────────

export const wireRecordFeedback = (
  senderEmail: string,
  action: string
): Promise<WireFeedbackResult> =>
  wireFetch(feedbackResultSchema, {
    body: { action, sender_email: senderEmail },
    method: 'POST',
    path: '/mail/feedback',
  })

// ── reactions ────────────────────────────────────────────────────

/**
 * `GET /api/mail/messages/{uid}/raw` — the message as it arrived.
 *
 * `message/rfc822`, not JSON, so it does not go through `wireFetch`:
 * that parses a body through a Zod schema and there is no schema for
 * "the bytes a sender sent". The headers a client normally hides are
 * the point — `Authentication-Results` is where the answer lives when
 * a message landed in Junk or claims to be from someone it is not.
 *
 * The route has been live since before this client existed and no web
 * page has ever called it; iOS gained a viewer for it this week.
 */
export async function wireGetMessageSource(uid: number): Promise<string> {
  // `getToken()`, not a localStorage key spelled again here — the
  // store owns where the token lives, and a second spelling is a
  // second thing to get wrong the day it moves.
  const token = getToken()
  const headers: Record<string, string> = {}
  if (token) headers.Authorization = `Bearer ${token}`
  const res = await fetch(`/api/mail/messages/${encodeURIComponent(String(uid))}/raw`, {
    headers,
  })
  if (!res.ok) throw new Error(`The server answered ${res.status}`)
  return res.text()
}

/**
 * Backend `get_thread_reactions` returns a flat
 * `{reactions: [{message_uid, emoji, count, me}, ...]}` — one row
 * per (uid, emoji) pair. Group by `message_uid` client-side for
 * per-message rendering.
 */
export async function wireGetThreadReactions(
  threadId: string
): Promise<Record<number, readonly WireReactionSummary[]>> {
  const raw = await wireFetch(threadReactionsSchema, {
    path: `/conversations/${encodeURIComponent(threadId)}/reactions`,
  })
  const grouped: Record<number, WireReactionSummary[]> = {}
  for (const r of raw.reactions) {
    const uid = r.message_uid ?? 0
    if (!grouped[uid]) grouped[uid] = []
    grouped[uid].push(r)
  }
  return grouped
}

export async function wireToggleReaction(
  threadId: string,
  uid: number,
  emoji: string
): Promise<readonly WireReactionSummary[]> {
  const raw = await wireFetch(reactionsListSchema, {
    body: { emoji },
    method: 'PUT',
    path: `/conversations/${encodeURIComponent(threadId)}/messages/${uid}/reactions`,
  })
  return raw.reactions
}

/**
 * `POST /api/mail/unsubscribe` — take the sender at their word.
 *
 * The server reads the URL out of the message's own `List-Unsubscribe`
 * header rather than taking one from this body, which is what stops
 * the endpoint being a request forwarder pointed at any URL a caller
 * names. So the request identifies the message, not the destination.
 *
 * Backend: `UnsubscribeRequest { thread_id, uid }` in
 * crates/webapi/src/handlers/unsubscribe.rs:24.
 */
export async function wireUnsubscribe(
  threadId: string,
  uid: number
): Promise<{ message?: string; ok: boolean; status?: number }> {
  return wireFetch(unsubscribeResultSchema, {
    body: { thread_id: threadId, uid },
    method: 'POST',
    path: '/mail/unsubscribe',
  })
}
