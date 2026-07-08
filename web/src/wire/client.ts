/**
 * Wire-layer transport client — layer 2 (see RFC §2.2).
 *
 * The ONE module in the app that talks to `fetch`. Callers pass a Zod
 * schema for the response; anything the wire returns is parsed through
 * that schema before it leaves this file. A parse failure is loud,
 * structured, and thrown as a `WireError.kind='validation'` — never
 * a silent `as any`.
 *
 * `fetchJson<T>` in `lib/api.ts` still exists during the migration; it
 * will be deleted once every screen reads through `wire/endpoints/`.
 */

import { z } from 'zod'

import { getToken } from '@/store/auth'

import { type WireError, WireErrorException } from './errors'

const API_BASE = '/api'

export type WireRequest = {
  /**
   * When true, a 204 No Content resolves as `undefined`. Otherwise
   * a 204 is a validation error against the schema.
   */
  readonly allowEmpty?: boolean
  readonly body?: unknown
  /**
   * When set, treat `body` as an already-encoded body (`FormData`,
   * `Blob`, `ReadableStream`, raw string). Skip `JSON.stringify`,
   * skip the `Content-Type: application/json` header — the browser
   * fills in the correct multipart boundary for `FormData`, and
   * caller can set custom headers via `extraHeaders`.
   */
  readonly bodyRaw?: BodyInit
  readonly extraHeaders?: Record<string, string>
  readonly method?: 'DELETE' | 'GET' | 'PATCH' | 'POST' | 'PUT'
  readonly path: string
  readonly signal?: AbortSignal
}

/**
 * Fetch a JSON endpoint and parse it through `schema`.
 * On any non-2xx, throw a structured `WireErrorException` — the caller
 * matches on `error.detail.kind`.
 */
export async function wireFetch<T>(schema: z.ZodType<T>, req: WireRequest): Promise<T> {
  const url = `${API_BASE}${req.path}`
  const headers: Record<string, string> = {}
  const token = getToken()
  if (token) headers.Authorization = `Bearer ${token}`
  let body: BodyInit | undefined
  if (req.bodyRaw !== undefined) {
    body = req.bodyRaw
  } else if (req.body !== undefined) {
    headers['Content-Type'] = 'application/json'
    body = JSON.stringify(req.body)
  }
  if (req.extraHeaders) {
    for (const [k, v] of Object.entries(req.extraHeaders)) headers[k] = v
  }

  let res: Response
  try {
    res = await fetch(url, {
      body,
      headers,
      method: req.method ?? 'GET',
      signal: req.signal,
    })
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') {
      throw new WireErrorException({ kind: 'aborted' })
    }
    throw new WireErrorException({ kind: 'network' })
  }

  return handleResponse(schema, res, Boolean(req.allowEmpty))
}

async function handleResponse<T>(
  schema: z.ZodType<T>,
  res: Response,
  allowEmpty: boolean
): Promise<T> {
  if (res.status === 401) throw new WireErrorException({ kind: 'auth' })
  if (res.status === 403) throw new WireErrorException({ kind: 'forbidden' })
  if (res.status === 404) throw new WireErrorException({ kind: 'not-found' })

  if (!res.ok) {
    let message = res.statusText || `HTTP ${res.status}`
    try {
      const body = await res.json()
      if (body && typeof body === 'object') {
        const asRecord = body as Record<string, unknown>
        if (typeof asRecord.error === 'string') message = asRecord.error
        else if (typeof asRecord.message === 'string') message = asRecord.message
      }
    } catch {
      /* body wasn't json; keep the default message */
    }
    const detail: WireError = { kind: 'server', message, status: res.status }
    throw new WireErrorException(detail)
  }

  if (res.status === 204) {
    if (allowEmpty) return undefined as T
    // 204 without allowEmpty means caller expected a body — parse
    // failure at the schema.
    const parsed = schema.safeParse(undefined)
    if (!parsed.success) {
      throw new WireErrorException({
        issues: parsed.error.issues,
        kind: 'validation',
      })
    }
    return parsed.data
  }

  const raw = await res.json().catch(() => undefined)
  const parsed = schema.safeParse(raw)
  if (!parsed.success) {
    throw new WireErrorException({
      issues: parsed.error.issues,
      kind: 'validation',
    })
  }
  return parsed.data
}
