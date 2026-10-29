import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { registerConfirmAsker } from '@/lib/confirm-asker'
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
  registerConfirmAsker(null)
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

  // the server wants the password typed again: ask once, then send the
  // same request again, reads included (an export or an audit read)
  it('asks for the password on confirm_required and repeats the request', async () => {
    const asker = vi.fn(async () => true)
    registerConfirmAsker(asker)
    const mock = vi
      .fn()
      .mockImplementationOnce(async () => refused('confirm_required'))
      .mockImplementationOnce(async () => new Response('{}'))
    vi.stubGlobal('fetch', mock)
    const res = await http('/api/admin/audit/accounts')
    expect(res.status).toBe(200)
    expect(asker).toHaveBeenCalledTimes(1)
    expect(mock).toHaveBeenCalledTimes(2)
  })

  it('returns the refusal untouched when the user cancels', async () => {
    registerConfirmAsker(async () => false)
    const mock = vi.fn(async () => refused('confirm_required'))
    vi.stubGlobal('fetch', mock)
    const res = await http('/api/agent/keys', { method: 'POST' })
    expect(res.status).toBe(403)
    expect(mock).toHaveBeenCalledTimes(1)
  })

  it('shows one dialog for requests refused at the same time', async () => {
    let finish: (confirmed: boolean) => void = () => {}
    const asker = vi.fn(() => new Promise<boolean>((done) => (finish = done)))
    registerConfirmAsker(asker)
    const mock = vi.fn(async (input: string) => {
      if (mock.mock.calls.filter((c) => c[0] === input).length === 1)
        return refused('confirm_required')
      return new Response('{}')
    })
    vi.stubGlobal('fetch', mock)
    const both = Promise.all([http('/api/a'), http('/api/b')])
    await vi.waitFor(() => expect(mock).toHaveBeenCalledTimes(2))
    await vi.waitFor(() => expect(asker).toHaveBeenCalled())
    finish(true)
    const [a, b] = await both
    expect([a.status, b.status]).toEqual([200, 200])
    expect(asker).toHaveBeenCalledTimes(1)
  })

  // the server restarting during a deploy: a read waits as told and goes
  // again once; a write is never repeated
  it('repeats a read once after Retry-After when the server is restarting', async () => {
    vi.useFakeTimers()
    try {
      const restarting = () => new Response('', { headers: { 'Retry-After': '2' }, status: 503 })
      const mock = vi
        .fn()
        .mockImplementationOnce(async () => restarting())
        .mockImplementationOnce(async () => new Response('{}'))
      vi.stubGlobal('fetch', mock)
      const pending = http('/api/conversations')
      await vi.advanceTimersByTimeAsync(1_999)
      expect(mock).toHaveBeenCalledTimes(1)
      await vi.advanceTimersByTimeAsync(1)
      expect((await pending).status).toBe(200)
      expect(mock).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('repeats a read only once', async () => {
    vi.useFakeTimers()
    try {
      const mock = vi.fn(
        async () => new Response('', { headers: { 'Retry-After': '1' }, status: 503 })
      )
      vi.stubGlobal('fetch', mock)
      const pending = http('/api/conversations')
      await vi.advanceTimersByTimeAsync(5_000)
      expect((await pending).status).toBe(503)
      expect(mock).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('never repeats a write the server could not take', async () => {
    const mock = vi.fn(
      async () => new Response('', { headers: { 'Retry-After': '1' }, status: 503 })
    )
    vi.stubGlobal('fetch', mock)
    const res = await http('/api/mail/send', { body: '{}', method: 'POST' })
    expect(res.status).toBe(503)
    expect(mock).toHaveBeenCalledTimes(1)
  })
})
