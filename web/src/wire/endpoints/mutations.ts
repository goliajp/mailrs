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

export function wireMarkAllRead(): Promise<WireMarkAllReadResponse> {
  return wireFetch(markAllReadResponseSchema, {
    body: {},
    method: 'POST',
    path: '/conversations/mark-all-read',
  })
}
