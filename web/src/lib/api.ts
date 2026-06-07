import { safeStorage } from '@/lib/safe-storage'
import { getToken } from '@/store/auth'

const API_BASE = '/api'

export type Draft = {
  bcc_addresses: string
  body: string
  cc_addresses: string
  created_at: string
  id: number
  reply_to_thread_id: null | string
  subject: string
  to_addresses: string
  updated_at: string
}

export type FeedbackAction =
  | 'archive'
  | 'block'
  | 'mark_important'
  | 'mark_spam'
  | 'mark_vip'
  | 'unblock'

// Optional validator/coercer applied to a parsed JSON response. Pass
// either a Zod schema's `.parse` (e.g. `HealthInfoSchema.parse`) or a
// lightweight shape assertion (e.g. `(raw) => assertArrayShape(...)`).
// Throws synchronously — caught by useQuery as a query error and
// surfaced through the route-level ErrorBoundary instead of a downstream
// `undefined.foo` deeper in the render tree.
export type ResponseValidator<T> = (raw: unknown) => T

export type SaveDraftRequest = {
  bcc?: string
  body?: string
  cc?: string
  reply_to_thread_id?: string
  subject?: string
  to?: string
}

type SaveDraftResult = {
  id?: number
  message?: string
  success: boolean
}

export async function deleteDraft(id: number): Promise<{ message?: string; success: boolean }> {
  return deleteJson<{ message?: string; success: boolean }>(`/mail/drafts/${id}`)
}

export async function deleteJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
    method: 'DELETE',
    signal,
  })
  return handleResponse<T>(res)
}

// --- draft types and API ---

export async function fetchBlob(path: string, signal?: AbortSignal): Promise<Blob> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
    signal,
  })
  if (res.status === 401) {
    safeStorage.removeItem('mailrs_auth')
    window.location.href = '/login'
    throw new Error('unauthorized')
  }
  if (!res.ok) {
    throw new Error(`Download failed: ${res.status}`)
  }
  return res.blob()
}

export async function fetchJson<T>(
  path: string,
  signal?: AbortSignal,
  validate?: ResponseValidator<T>
): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
    signal,
  })
  const data = await handleResponse<T>(res)
  return validate ? validate(data) : data
}

export async function getThreadReactions(
  threadId: string
): Promise<Record<number, ReactionSummary[]>> {
  const result = await fetchJson<{
    reactions: Record<number, ReactionSummary[]>
  }>(`/conversations/${encodeURIComponent(threadId)}/reactions`)
  return result.reactions
}

export async function listDrafts(): Promise<Draft[]> {
  return fetchJson<Draft[]>('/mail/drafts')
}

export async function postJson<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    body: JSON.stringify(body),
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    method: 'POST',
    signal,
  })
  return handleResponse<T>(res)
}

export async function putJson<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    body: JSON.stringify(body),
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    method: 'PUT',
    signal,
  })
  return handleResponse<T>(res)
}

// --- reactions API ---

import type { ReactionSummary } from '@/lib/types'

export async function recordFeedback(
  senderEmail: string,
  action: FeedbackAction
): Promise<{ message?: string; success: boolean }> {
  return postJson('/mail/feedback', { action, sender_email: senderEmail })
}

export async function saveDraft(draft: SaveDraftRequest): Promise<SaveDraftResult> {
  return postJson<SaveDraftResult>('/mail/drafts', draft)
}

// --- snooze API ---

export async function snoozeConversation(
  threadId: string,
  until: string
): Promise<{ message?: string; success: boolean }> {
  return putJson(`/conversations/${encodeURIComponent(threadId)}/snooze`, {
    until,
  })
}

export async function toggleReaction(
  threadId: string,
  uid: number,
  emoji: string
): Promise<ReactionSummary[]> {
  const result = await putJson<{ reactions: ReactionSummary[] }>(
    `/conversations/${encodeURIComponent(threadId)}/messages/${uid}/reactions`,
    { emoji }
  )
  return result.reactions
}

// --- sender feedback API ---

export async function unsnoozeConversation(
  threadId: string
): Promise<{ message?: string; success: boolean }> {
  return deleteJson(`/conversations/${encodeURIComponent(threadId)}/snooze`)
}

function authHeaders(): Record<string, string> {
  const token = getToken()
  if (token) return { Authorization: `Bearer ${token}` }
  return {}
}

async function handleResponse<T>(res: Response): Promise<T> {
  if (res.status === 401) {
    safeStorage.removeItem('mailrs_auth')
    window.location.href = '/login'
    throw new Error('unauthorized')
  }
  if (!res.ok) {
    let message = `API error: ${res.status}`
    try {
      const body = await res.json()
      if (body?.error) message = body.error
      else if (body?.message) message = body.message
    } catch {
      // response body not json, use default message
    }
    throw new Error(message)
  }
  return res.json()
}
