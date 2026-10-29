import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { http } from '@/lib/http'

function refused(error: string): Response {
  return new Response(JSON.stringify({ error }), { status: 403 })
}

function sentToken(mock: ReturnType<typeof vi.fn>, call: number): null | string {
  const init = (mock.mock.calls[call] as unknown[])[1] as RequestInit | undefined
  return new Headers(init?.headers).get('X-CSRF-Token')
}

beforeEach(() => {
  document.cookie = 'mailrs_csrf=tok-1; path=/'
})

afterEach(() => {
  vi.unstubAllGlobals()
  document.cookie = 'mailrs_csrf=; max-age=0; path=/'
})

describe('http', () => {
  it('sends the token on a write and leaves other headers alone', async () => {
    const mock = vi.fn(async () => new Response('{}'))
    vi.stubGlobal('fetch', mock)
    await http('/api/x', { headers: { 'Content-Type': 'application/json' }, method: 'POST' })
    expect(sentToken(mock, 0)).toBe('tok-1')
    const init = (mock.mock.calls[0] as unknown[])[1] as RequestInit
    expect(new Headers(init.headers).get('Content-Type')).toBe('application/json')
  })

  it('sends nothing extra on a read', async () => {
    const mock = vi.fn(async () => new Response('{}'))
    vi.stubGlobal('fetch', mock)
    await http('/api/x')
    await http('/api/y', { method: 'GET' })
    expect(mock.mock.calls[0]).toEqual(['/api/x'])
    expect(sentToken(mock, 1)).toBeNull()
  })

  // a session from before the token existed is refused once, and gets the
  // cookie on that refusal
  it('retries once, with the new cookie, when the token was missing', async () => {
    const mock = vi
      .fn()
      .mockImplementationOnce(async () => {
        document.cookie = 'mailrs_csrf=tok-2; path=/'
        return refused('csrf_missing')
      })
      .mockImplementationOnce(async () => new Response('{}'))
    vi.stubGlobal('fetch', mock)
    const res = await http('/api/x', { method: 'DELETE' })
    expect(res.status).toBe(200)
    expect(mock).toHaveBeenCalledTimes(2)
    expect(sentToken(mock, 1)).toBe('tok-2')
  })

  it('does not retry any other refusal, and retries at most once', async () => {
    const mock = vi.fn(async () => refused('csrf_mismatch'))
    vi.stubGlobal('fetch', mock)
    expect((await http('/api/x', { method: 'POST' })).status).toBe(403)
    expect(mock).toHaveBeenCalledTimes(1)

    const missing = vi.fn(async () => refused('csrf_missing'))
    vi.stubGlobal('fetch', missing)
    expect((await http('/api/x', { method: 'POST' })).status).toBe(403)
    expect(missing).toHaveBeenCalledTimes(2)
  })
})
