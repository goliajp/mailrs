import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  deleteDraft,
  deleteJson,
  fetchBlob,
  fetchJson,
  getThreadReactions,
  listDrafts,
  postJson,
  putJson,
  recordFeedback,
  saveDraft,
  snoozeConversation,
  toggleReaction,
  unsnoozeConversation,
} from '../api'

// mock getToken from auth store
vi.mock('@/store/auth', () => ({
  getToken: vi.fn(),
}))

import { getToken } from '@/store/auth'

const mockGetToken = vi.mocked(getToken)

function makeFetchMock(status: number, body: unknown, isJson = true): typeof fetch {
  return vi.fn().mockResolvedValue({
    blob: vi.fn().mockResolvedValue(body instanceof Blob ? body : new Blob()),
    json: isJson
      ? vi.fn().mockResolvedValue(body)
      : vi.fn().mockRejectedValue(new SyntaxError('not json')),
    ok: status >= 200 && status < 300,
    status,
  } as unknown as Response)
}

describe('authHeaders', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', makeFetchMock(200, { ok: true }))
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('includes Authorization header when token is present', async () => {
    mockGetToken.mockReturnValue('test-token-abc')
    await fetchJson('/test')
    const call = vi.mocked(fetch).mock.calls[0]
    const opts = call[1] as RequestInit
    expect((opts.headers as Record<string, string>)['Authorization']).toBe('Bearer test-token-abc')
  })

  it('omits Authorization header when token is null', async () => {
    mockGetToken.mockReturnValue(null)
    await fetchJson('/test')
    const call = vi.mocked(fetch).mock.calls[0]
    const opts = call[1] as RequestInit
    expect((opts.headers as Record<string, string>)['Authorization']).toBeUndefined()
  })
})

describe('fetchJson', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('calls fetch with correct URL and returns parsed JSON', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(200, { data: 'hello' }))
    const result = await fetchJson<{ data: string }>('/conversations')
    expect(result).toEqual({ data: 'hello' })
    expect(vi.mocked(fetch)).toHaveBeenCalledWith('/api/conversations', expect.any(Object))
  })

  it('passes AbortSignal to fetch', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(200, {}))
    const controller = new AbortController()
    await fetchJson('/test', controller.signal)
    const opts = vi.mocked(fetch).mock.calls[0][1] as RequestInit
    expect(opts.signal).toBe(controller.signal)
  })

  it('throws on 401 and redirects to /login with return_to', async () => {
    mockGetToken.mockReturnValue('stale-token')
    const removeItem = vi.fn()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(),
      removeItem,
      setItem: vi.fn(),
    })
    vi.stubGlobal('window', {
      ...globalThis.window,
      location: { hash: '', href: '', pathname: '/mail/inbox', search: '?folder=Inbox' },
    })
    vi.stubGlobal('fetch', makeFetchMock(401, {}))
    await expect(fetchJson('/secure')).rejects.toThrow('unauthorized')
    expect(removeItem).toHaveBeenCalledWith('mailrs_auth')
    expect(window.location.href).toBe(
      '/login?return_to=' + encodeURIComponent('/mail/inbox?folder=Inbox')
    )
  })

  it('does not redirect when 401 fires from /login itself (avoid loop)', async () => {
    mockGetToken.mockReturnValue('stale-token')
    const removeItem = vi.fn()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(),
      removeItem,
      setItem: vi.fn(),
    })
    vi.stubGlobal('window', {
      ...globalThis.window,
      location: { hash: '', href: '/login', pathname: '/login', search: '' },
    })
    vi.stubGlobal('fetch', makeFetchMock(401, {}))
    await expect(fetchJson('/secure')).rejects.toThrow('unauthorized')
    expect(removeItem).toHaveBeenCalledWith('mailrs_auth')
    // href left untouched — we're already on /login
    expect(window.location.href).toBe('/login')
  })

  it('throws with server error message from body.error', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(422, { error: 'validation failed' }))
    await expect(fetchJson('/test')).rejects.toThrow('validation failed')
  })

  it('throws with server error message from body.message', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(500, { message: 'internal error' }))
    await expect(fetchJson('/test')).rejects.toThrow('internal error')
  })

  it('throws generic message when error body is not JSON', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(503, {}, false))
    await expect(fetchJson('/test')).rejects.toThrow('API error: 503')
  })
})

describe('postJson', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends POST with JSON body and Content-Type header', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(200, { success: true }))
    const payload = { subject: 'hello', to: 'user@example.com' }
    const result = await postJson<{ success: boolean }>('/mail/send', payload)
    expect(result).toEqual({ success: true })
    const call = vi.mocked(fetch).mock.calls[0]
    const opts = call[1] as RequestInit
    expect(opts.method).toBe('POST')
    expect((opts.headers as Record<string, string>)['Content-Type']).toBe('application/json')
    expect(opts.body).toBe(JSON.stringify(payload))
  })

  it('includes auth header when token exists', async () => {
    mockGetToken.mockReturnValue('my-token')
    vi.stubGlobal('fetch', makeFetchMock(200, {}))
    await postJson('/test', {})
    const opts = vi.mocked(fetch).mock.calls[0][1] as RequestInit
    expect((opts.headers as Record<string, string>)['Authorization']).toBe('Bearer my-token')
  })
})

describe('putJson', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends PUT with JSON body', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(200, { updated: true }))
    const result = await putJson<{ updated: boolean }>('/resource/1', {
      name: 'test',
    })
    expect(result).toEqual({ updated: true })
    const opts = vi.mocked(fetch).mock.calls[0][1] as RequestInit
    expect(opts.method).toBe('PUT')
    expect((opts.headers as Record<string, string>)['Content-Type']).toBe('application/json')
  })
})

describe('deleteJson', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends DELETE request and returns parsed JSON', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(200, { success: true }))
    const result = await deleteJson<{ success: boolean }>('/resource/42')
    expect(result).toEqual({ success: true })
    const opts = vi.mocked(fetch).mock.calls[0][1] as RequestInit
    expect(opts.method).toBe('DELETE')
  })
})

describe('fetchBlob', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('returns Blob on success', async () => {
    mockGetToken.mockReturnValue(null)
    const blob = new Blob(['binary'], { type: 'application/pdf' })
    const fetchMock = vi.fn().mockResolvedValue({
      blob: vi.fn().mockResolvedValue(blob),
      ok: true,
      status: 200,
    } as unknown as Response)
    vi.stubGlobal('fetch', fetchMock)
    const result = await fetchBlob('/attachment/1')
    expect(result).toBe(blob)
    expect(fetchMock).toHaveBeenCalledWith('/api/attachment/1', expect.any(Object))
  })

  it('throws on 401 and redirects to /login with return_to', async () => {
    mockGetToken.mockReturnValue('old-token')
    const removeItem = vi.fn()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(),
      removeItem,
      setItem: vi.fn(),
    })
    vi.stubGlobal('window', {
      ...globalThis.window,
      location: { hash: '', href: '', pathname: '/mail/conv/abc', search: '' },
    })
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        blob: vi.fn(),
        ok: false,
        status: 401,
      } as unknown as Response)
    )
    await expect(fetchBlob('/attachment/1')).rejects.toThrow('unauthorized')
    expect(removeItem).toHaveBeenCalledWith('mailrs_auth')
    expect(window.location.href).toBe('/login?return_to=' + encodeURIComponent('/mail/conv/abc'))
  })

  it('throws on non-200 non-401 status', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        blob: vi.fn(),
        ok: false,
        status: 404,
      } as unknown as Response)
    )
    await expect(fetchBlob('/attachment/99')).rejects.toThrow('Download failed: 404')
  })
})

describe('deleteDraft', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends DELETE to /mail/drafts/:id and returns result', async () => {
    mockGetToken.mockReturnValue('tok')
    vi.stubGlobal('fetch', makeFetchMock(200, { success: true }))
    const result = await deleteDraft(42)
    expect(result).toEqual({ success: true })
    const call = vi.mocked(fetch).mock.calls[0]
    expect(call[0]).toBe('/api/mail/drafts/42')
    expect((call[1] as RequestInit).method).toBe('DELETE')
  })
})

describe('getThreadReactions', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('fetches reactions and returns the reactions record', async () => {
    mockGetToken.mockReturnValue(null)
    const reactions = { 1: [{ count: 2, emoji: '👍', reacted: true }] }
    vi.stubGlobal('fetch', makeFetchMock(200, { reactions }))
    const result = await getThreadReactions('thread-abc')
    expect(result).toEqual(reactions)
    expect(vi.mocked(fetch).mock.calls[0][0]).toBe('/api/conversations/thread-abc/reactions')
  })

  it('encodes threadId in URL', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', makeFetchMock(200, { reactions: {} }))
    await getThreadReactions('thread with spaces')
    expect(vi.mocked(fetch).mock.calls[0][0]).toBe(
      '/api/conversations/thread%20with%20spaces/reactions'
    )
  })
})

describe('listDrafts', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('fetches drafts list via GET', async () => {
    mockGetToken.mockReturnValue(null)
    const drafts = [{ id: 1, subject: 'Draft 1' }]
    vi.stubGlobal('fetch', makeFetchMock(200, drafts))
    const result = await listDrafts()
    expect(result).toEqual(drafts)
    expect(vi.mocked(fetch).mock.calls[0][0]).toBe('/api/mail/drafts')
  })
})

describe('recordFeedback', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends POST with sender_email and action', async () => {
    mockGetToken.mockReturnValue('tok')
    vi.stubGlobal('fetch', makeFetchMock(200, { success: true }))
    const result = await recordFeedback('spam@example.com', 'mark_spam')
    expect(result).toEqual({ success: true })
    const call = vi.mocked(fetch).mock.calls[0]
    expect(call[0]).toBe('/api/mail/feedback')
    const opts = call[1] as RequestInit
    expect(opts.method).toBe('POST')
    expect(JSON.parse(opts.body as string)).toEqual({
      action: 'mark_spam',
      sender_email: 'spam@example.com',
    })
  })
})

describe('saveDraft', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends POST with draft payload and returns result', async () => {
    mockGetToken.mockReturnValue('tok')
    const draft = { body: 'Hello', subject: 'Hi', to: ['a@b.com'] }
    vi.stubGlobal('fetch', makeFetchMock(200, { id: 1, success: true }))
    const result = await saveDraft(draft as never)
    expect(result).toEqual({ id: 1, success: true })
    const call = vi.mocked(fetch).mock.calls[0]
    expect(call[0]).toBe('/api/mail/drafts')
    expect((call[1] as RequestInit).method).toBe('POST')
  })
})

describe('snoozeConversation', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends PUT with until param', async () => {
    mockGetToken.mockReturnValue('tok')
    vi.stubGlobal('fetch', makeFetchMock(200, { success: true }))
    const result = await snoozeConversation('thread-1', '2024-12-01T09:00:00Z')
    expect(result).toEqual({ success: true })
    const call = vi.mocked(fetch).mock.calls[0]
    expect(call[0]).toBe('/api/conversations/thread-1/snooze')
    const opts = call[1] as RequestInit
    expect(opts.method).toBe('PUT')
    expect(JSON.parse(opts.body as string)).toEqual({
      until: '2024-12-01T09:00:00Z',
    })
  })
})

describe('toggleReaction', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends PUT with emoji and returns reactions array', async () => {
    mockGetToken.mockReturnValue('tok')
    const reactions = [{ count: 1, emoji: '👍', reacted: true }]
    vi.stubGlobal('fetch', makeFetchMock(200, { reactions }))
    const result = await toggleReaction('thread-1', 5, '👍')
    expect(result).toEqual(reactions)
    const call = vi.mocked(fetch).mock.calls[0]
    expect(call[0]).toBe('/api/conversations/thread-1/messages/5/reactions')
    expect((call[1] as RequestInit).method).toBe('PUT')
  })
})

describe('unsnoozeConversation', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    mockGetToken.mockReset()
  })

  it('sends DELETE to snooze endpoint', async () => {
    mockGetToken.mockReturnValue('tok')
    vi.stubGlobal('fetch', makeFetchMock(200, { success: true }))
    const result = await unsnoozeConversation('thread-2')
    expect(result).toEqual({ success: true })
    const call = vi.mocked(fetch).mock.calls[0]
    expect(call[0]).toBe('/api/conversations/thread-2/snooze')
    expect((call[1] as RequestInit).method).toBe('DELETE')
  })
})
