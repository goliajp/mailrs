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
    redirectToLogin()
    throw new Error('unauthorized')
  }
  if (!res.ok) {
    throw new Error(`Download failed: ${res.status}`)
  }
  return res.blob()
}

export async function fetchJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
    signal,
  })
  return handleResponse<T>(res)
}

/**
 * List-endpoint reader that hides the "bare array or `{items:[]}` envelope"
 * ambiguity from every caller.
 *
 * Every admin list endpoint on the mailrs webapi wraps its payload in
 * `{ items: [...] }` (`crates/core-api/src/method/admin.rs` ->
 * `*ListResponse`). Historically some monolith endpoints returned a bare
 * array, and the frontend was written against the bare shape. When the
 * fastcore-native handlers took over, every admin page hit `TypeError:
 * X.map is not a function` because the wire shape shifted under the
 * lie the `fetchJson<X[]>` type had been telling.
 *
 * This helper collapses every plausible shape a list endpoint might
 * return — bare array, `{items}`, `{results}`, `{data}`, `{list}`, and
 * whatever `wire::*ListResponse` exposes as its single-array-field name
 * — into a plain `T[]`. The single-array-field discovery is important:
 * the webapi's `WebhookListResponse` might tomorrow rename `items` to
 * `webhooks` and this helper will still work.
 *
 * On anything that isn't parseable as a list (a 401 body echoed as
 * data, a `{error}` object, a null), the helper resolves to `[]` rather
 * than throwing — the page renders "No entries" instead of unmounting.
 *
 * **All future admin queries MUST use `fetchList<T>()`, not
 * `fetchJson<T[]>()`.** The bare shape is a liability.
 */
export async function fetchList<T>(path: string, signal?: AbortSignal): Promise<T[]> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
    signal,
  })
  if (res.status === 401) {
    redirectToLogin()
    throw new Error('unauthorized')
  }
  // 204 No Content is a legitimate "empty list" from endpoints like
  // /api/icon that intentionally avoid 404s. Anything non-2xx else is
  // still a hard error the caller should see.
  if (res.status === 204) return []
  if (!res.ok) throw new Error(`API error: ${res.status}`)
  // Some servers hand back `application/json` with an empty body on
  // 200 (uncommon, but possible with reverse proxies). Treat it as [].
  const text = await res.text()
  if (text.length === 0) return []
  return unwrapList<T>(JSON.parse(text))
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
  return fetchList<Draft>('/mail/drafts')
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

export async function recordFeedback(
  senderEmail: string,
  action: FeedbackAction
): Promise<{ message?: string; success: boolean }> {
  return postJson('/mail/feedback', { action, sender_email: senderEmail })
}

// --- reactions API ---

import type { ReactionSummary } from '@/lib/types'

export async function saveDraft(draft: SaveDraftRequest): Promise<SaveDraftResult> {
  return postJson<SaveDraftResult>('/mail/drafts', draft)
}

export async function snoozeConversation(
  threadId: string,
  until: string
): Promise<{ message?: string; success: boolean }> {
  return putJson(`/conversations/${encodeURIComponent(threadId)}/snooze`, {
    until,
  })
}

// --- snooze API ---

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

export async function unsnoozeConversation(
  threadId: string
): Promise<{ message?: string; success: boolean }> {
  return deleteJson(`/conversations/${encodeURIComponent(threadId)}/snooze`)
}

// --- sender feedback API ---

/**
 * Public for testing. Extracts a `T[]` from whatever shape a list
 * endpoint might return. See `fetchList` for the full list of shapes.
 */
export function unwrapList<T>(raw: unknown): T[] {
  if (Array.isArray(raw)) return raw as T[]
  if (raw == null || typeof raw !== 'object') return []
  const obj = raw as Record<string, unknown>
  // Common envelope keys first (fast path — matches every current
  // wire::*ListResponse shape and every historical alternative).
  for (const key of ['items', 'results', 'data', 'list', 'rows']) {
    if (Array.isArray(obj[key])) return obj[key] as T[]
  }
  // Fallback: pick the single array-valued property. Handles a future
  // wire type that names its list field after the resource (e.g.
  // `{ webhooks: [...] }`) without needing this helper to know the
  // resource name.
  const arrayValues = Object.values(obj).filter(Array.isArray)
  if (arrayValues.length === 1) return arrayValues[0] as T[]
  return []
}

function authHeaders(): Record<string, string> {
  const token = getToken()
  if (token) return { Authorization: `Bearer ${token}` }
  return {}
}

async function handleResponse<T>(res: Response): Promise<T> {
  if (res.status === 401) {
    redirectToLogin()
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
  // 204 No Content (and any other empty-body success) — many mutation
  // endpoints (star / unstar / pin / archive / mark-unread / mark-read /
  // dismiss-action / snooze delete / etc.) return 204 with zero content
  // length. Calling `res.json()` on an empty body throws
  // `SyntaxError: Unexpected end of JSON input`, which then fires the
  // mutation's onError (rollback + toast). Return undefined instead —
  // every current caller of postJson/putJson/deleteJson for a 204
  // endpoint ignores the return value.
  if (res.status === 204 || res.headers?.get?.('Content-Length') === '0') {
    return undefined as T
  }
  return res.json()
}

// Drop the stale token and bounce to /login, but preserve the current
// URL via ?return_to= so the user lands back on the same view after
// re-authenticating. The login page already honours return_to.
function redirectToLogin(): void {
  if (typeof window === 'undefined') return
  localStorage.removeItem('mailrs_auth')
  const here = window.location.pathname + window.location.search + window.location.hash
  // Don't loop if we're already on /login (avoid replacing return_to of
  // an in-flight login attempt with itself).
  if (window.location.pathname === '/login') {
    return
  }
  window.location.href = `/login?return_to=${encodeURIComponent(here)}`
}
