import { getToken } from '@/store/auth'

const API_BASE = '/api'

function authHeaders(): Record<string, string> {
  const token = getToken()
  if (token) return { Authorization: `Bearer ${token}` }
  return {}
}

async function handleResponse<T>(res: Response): Promise<T> {
  if (res.status === 401) {
    localStorage.removeItem('mailrs_auth')
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

export async function fetchJson<T>(
  path: string,
  signal?: AbortSignal
): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
    signal,
  })
  return handleResponse<T>(res)
}

export async function postJson<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  })
  return handleResponse<T>(res)
}

export async function putJson<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  })
  return handleResponse<T>(res)
}

export async function deleteJson<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: 'DELETE',
    headers: authHeaders(),
  })
  return handleResponse<T>(res)
}

export async function fetchBlob(path: string): Promise<Blob> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
  })
  if (res.status === 401) {
    localStorage.removeItem('mailrs_auth')
    window.location.href = '/login'
    throw new Error('unauthorized')
  }
  if (!res.ok) {
    throw new Error(`Download failed: ${res.status}`)
  }
  return res.blob()
}

// --- draft types and API ---

export type Draft = {
  id: number
  to_addresses: string
  cc_addresses: string
  bcc_addresses: string
  subject: string
  body: string
  reply_to_thread_id: string | null
  created_at: string
  updated_at: string
}

export type SaveDraftRequest = {
  to?: string
  cc?: string
  bcc?: string
  subject?: string
  body?: string
  reply_to_thread_id?: string
}

type SaveDraftResult = {
  success: boolean
  id?: number
  message?: string
}

export async function saveDraft(
  draft: SaveDraftRequest,
): Promise<SaveDraftResult> {
  return postJson<SaveDraftResult>('/mail/drafts', draft)
}

export async function listDrafts(): Promise<Draft[]> {
  return fetchJson<Draft[]>('/mail/drafts')
}

export async function deleteDraft(
  id: number,
): Promise<{ success: boolean; message?: string }> {
  return deleteJson<{ success: boolean; message?: string }>(
    `/mail/drafts/${id}`,
  )
}
