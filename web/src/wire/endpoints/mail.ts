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
  snoozeResultSchema,
  threadReactionsSchema,
  type WireDeleteDraftResult,
  type WireDraft,
  type WireFeedbackResult,
  type WireInlineUploadResult,
  type WireReactionSummary,
  type WireSaveDraftResult,
  type WireSendResult,
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

export const wireSnoozeConversation = (
  threadId: string,
  until: string
): Promise<WireSnoozeResult> =>
  wireFetch(snoozeResultSchema, {
    body: { until },
    method: 'PUT',
    path: `/conversations/${encodeURIComponent(threadId)}/snooze`,
  })

export const wireUnsnoozeConversation = (threadId: string): Promise<WireSnoozeResult> =>
  wireFetch(snoozeResultSchema, {
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

export const wireSaveDraft = (payload: Record<string, unknown>): Promise<WireSaveDraftResult> =>
  wireFetch(saveDraftResultSchema, {
    body: payload,
    method: 'POST',
    path: '/mail/drafts',
  })

export const wireDeleteDraft = (id: number): Promise<WireDeleteDraftResult> =>
  wireFetch(deleteDraftResultSchema, {
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

export async function wireGetThreadReactions(
  threadId: string
): Promise<Record<string, readonly WireReactionSummary[]>> {
  const raw = await wireFetch(threadReactionsSchema, {
    path: `/conversations/${encodeURIComponent(threadId)}/reactions`,
  })
  return raw.reactions
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
