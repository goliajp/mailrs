/**
 * The one place this app calls `fetch`.
 *
 * The session is an HttpOnly cookie the browser attaches to every request,
 * including one a page on another site makes it send. So a request that
 * changes something must also carry the session's CSRF token in a header,
 * which another site can neither read nor set. The server hands the token
 * out as the readable `mailrs_csrf` cookie.
 *
 * A session from before the token existed gets its cookie on the response
 * that refuses it (`csrf_missing`); the request is then sent once more with
 * the token.
 *
 * An operation that needs a recent password confirmation is refused with
 * `confirm_required`; the user is asked for the password and the request
 * is sent once more.
 *
 * A read answered 503 — the server restarting during a deploy — is sent
 * once more after the `Retry-After` it gives. A write is not: sending,
 * moving or deleting twice could do it twice, so the caller says the
 * server is restarting and leaves the retry to the person. Everything else
 * is the caller's: status handling, parsing.
 */

import { askToConfirm } from './confirm-asker'

const CSRF_COOKIE = 'mailrs_csrf'
const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS'])
const MAX_WAIT_MS = 30_000

export async function http(input: string, init?: RequestInit): Promise<Response> {
  let res = await send(input, init)
  if (res.status === 503 && SAFE_METHODS.has((init?.method ?? 'GET').toUpperCase())) {
    await new Promise((done) => setTimeout(done, retryAfterMs(res)))
    res = await send(input, init)
  }
  if (res.status !== 403 || (await errorCode(res)) !== 'confirm_required') return res
  if (!(await askToConfirm())) return res
  return send(input, init)
}

function csrfToken(): null | string {
  for (const part of document.cookie.split(';')) {
    const [name, ...rest] = part.trim().split('=')
    if (name === CSRF_COOKIE) return rest.join('=')
  }
  return null
}

async function errorCode(res: Response): Promise<unknown> {
  const body: unknown = await res
    .clone()
    .json()
    .catch(() => null)
  if (typeof body !== 'object' || body === null) return null
  return (body as { error?: unknown }).error
}

function retryAfterMs(res: Response): number {
  const seconds = Number(res.headers.get('Retry-After'))
  if (!Number.isFinite(seconds) || seconds <= 0) return 5_000
  return Math.min(seconds * 1000, MAX_WAIT_MS)
}

async function send(input: string, init?: RequestInit): Promise<Response> {
  if (init === undefined) return fetch(input)
  const method = (init.method ?? 'GET').toUpperCase()
  if (SAFE_METHODS.has(method)) return fetch(input, init)
  const first = await fetch(input, withToken(init))
  if (first.status !== 403 || (await errorCode(first)) !== 'csrf_missing') return first
  return fetch(input, withToken(init))
}

function withToken(init: RequestInit): RequestInit {
  const headers = new Headers(init.headers)
  const token = csrfToken()
  if (token) headers.set('X-CSRF-Token', token)
  return { ...init, headers }
}
