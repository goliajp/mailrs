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
export const wireSnoozeConversation = (
  threadId: string,
  until: string
): Promise<undefined | WireSnoozeResult> =>
  wireFetch(snoozeResultSchema.optional(), {
    allowEmpty: true,
    body: { until },
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

export async function wireDeletePendingSend(messageId: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/mail/pending/${encodeURIComponent(messageId)}`,
  })
}

// ── /mail/drafts ──────────────────────────────────────────────────

export async function wireListDrafts(): Promise<readonly WireDraft[]> {
  const raw = await wireFetch(draftListSchema, { path: '/mail/drafts' })
  return raw.items
}

export function wireListSentMessages(): Promise<readonly WireSentMessage[]> {
  return wireFetch(sentMessagesSchema, { path: '/mail/sent' })
}

export const wireSaveDraft = (payload: Record<string, unknown>): Promise<WireSaveDraftResult> =>
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
