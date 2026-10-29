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
 * the token. Everything else is the caller's: status handling, parsing.
 */

const CSRF_COOKIE = 'mailrs_csrf'
const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS'])

export async function http(input: string, init?: RequestInit): Promise<Response> {
  if (init === undefined) return fetch(input)
  const method = (init.method ?? 'GET').toUpperCase()
  if (SAFE_METHODS.has(method)) return fetch(input, init)
  const first = await fetch(input, withToken(init))
  if (first.status !== 403 || !(await refusedForMissingToken(first))) return first
  return fetch(input, withToken(init))
}

function csrfToken(): null | string {
  for (const part of document.cookie.split(';')) {
    const [name, ...rest] = part.trim().split('=')
    if (name === CSRF_COOKIE) return rest.join('=')
  }
  return null
}

async function refusedForMissingToken(res: Response): Promise<boolean> {
  const body: unknown = await res
    .clone()
    .json()
    .catch(() => null)
  if (typeof body !== 'object' || body === null) return false
  return (body as { error?: unknown }).error === 'csrf_missing'
}

function withToken(init: RequestInit): RequestInit {
  const headers = new Headers(init.headers)
  const token = csrfToken()
  if (token) headers.set('X-CSRF-Token', token)
  return { ...init, headers }
}
