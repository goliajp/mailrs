/**
 * Thread-mutation wire endpoints — v2.1 §7 batch 1 (2026-07-08).
 *
 * Every write on a single thread flows through one of these thin
 * adapters. Each returns `Promise<void>` for the 204 shapes so
 * mutations that don't care about the response body have nothing
 * to await. Batch and mark-all-read return structured responses.
 *
 * The adapters DON'T couple to react-query — the caller (typically
 * `use-mail-mutations.ts` inside a `useMutation({mutationFn})`) owns
 * the optimistic patch + rollback lifecycle. This file just handles
 * "issue the request, parse the response, throw structured errors".
 */

import { wireFetch } from '../client'
import {
  batchMutationResponseSchema,
  emptyResponseSchema,
  markAllReadResponseSchema,
  type WireBatchMutationResponse,
  type WireMarkAllReadResponse,
} from '../schemas/mutations'

// ── single-thread 204 mutations ────────────────────────────────────

async function postEmpty(path: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: {},
    method: 'POST',
    path,
  })
}

export const wireArchiveThread = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/archive`)

export const wireUnarchiveThread = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/unarchive`)

export const wireStarThread = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/star`)

export const wireUnstarThread = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/unstar`)

export const wirePinThread = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/pin`)

export const wireUnpinThread = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/unpin`)

export const wireMarkThreadRead = (threadId: string, domains?: string[]): Promise<void> => {
  const q = domains && domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
  return postEmpty(`/conversations/${encodeURIComponent(threadId)}/read${q}`)
}

export const wireMarkThreadUnread = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/unread`)

// v2.4.1 Phase 3 (RFC-B §3.4) — mark-junk / mark-not-junk.
// `mark-not-junk` auto-populates the recipient's whitelist with the
// thread's senders; both endpoints move the thread between the Inbox
// and Junk top-level folder zsets.
export const wireMarkJunk = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/mark-junk`)

export const wireMarkNotJunk = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/mark-not-junk`)

// v2.9 triage — move a thread between the Inbox / Notifications /
// Promotions buckets. Each move also trains the multi-class classifier
// on the correction (backend side). 204/empty responses.
export const wireMarkNotification = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/mark-notification`)

export const wireMarkPromotion = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/mark-promotion`)

export const wireMoveToInbox = (threadId: string): Promise<void> =>
  postEmpty(`/conversations/${encodeURIComponent(threadId)}/move-to-inbox`)

export function wireBatchMutation(
  action: string,
  threadIds: string[]
): Promise<WireBatchMutationResponse> {
  return wireFetch(batchMutationResponseSchema, {
    body: { action, thread_ids: threadIds },
    method: 'POST',
    path: '/conversations/batch',
  })
}

// ── multi-thread aggregated mutations ────────────────────────────

export async function wireDeleteThread(threadId: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/conversations/${encodeURIComponent(threadId)}`,
  })
}

/**
 * Mark read what the current list is showing.
 *
 * The axes go in the query string — the same ones the conversation
 * list sends — and the server marks exactly what that query returns,
 * including the rows this client has not scrolled to. Called with no
 * axes it is the whole mailbox, which is what this did before the
 * route learned to scope (2026-08-12) and what the name still says.
 *
 * Scoped server-side rather than by sending thread ids: a client can
 * only name the page it has loaded, and marking 50 of 1,458 would look
 * finished and do a fraction.
 */
export function wireMarkAllRead(axes?: {
  archived?: boolean
  folder?: null | string
  starred?: boolean
  unread?: boolean
}): Promise<WireMarkAllReadResponse> {
  const query = new URLSearchParams()
  if (axes?.folder) query.set('folder', axes.folder)
  if (axes?.archived) query.set('archived', 'true')
  if (axes?.starred) query.set('starred', 'true')
  if (axes?.unread) query.set('unread', 'true')
  const suffix = query.size > 0 ? `?${query}` : ''
  return wireFetch(markAllReadResponseSchema, {
    body: {},
    method: 'POST',
    path: `/conversations/mark-all-read${suffix}`,
  })
}
