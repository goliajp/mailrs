import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  deleteJson,
  fetchBlob,
  fetchJson,
  postJson,
  putJson,
} from '../api'

// mock getToken from auth store
vi.mock('@/store/auth', () => ({
  getToken: vi.fn(),
}))

import { getToken } from '@/store/auth'

const mockGetToken = vi.mocked(getToken)

function makeFetchMock(
  status: number,
  body: unknown,
  isJson = true,
): typeof fetch {
  return vi.fn().mockResolvedValue({
    status,
    ok: status >= 200 && status < 300,
    json: isJson
      ? vi.fn().mockResolvedValue(body)
      : vi.fn().mockRejectedValue(new SyntaxError('not json')),
    blob: vi.fn().mockResolvedValue(body instanceof Blob ? body : new Blob()),
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
    expect((opts.headers as Record<string, string>)['Authorization']).toBe(
      'Bearer test-token-abc',
    )
  })

  it('omits Authorization header when token is null', async () => {
    mockGetToken.mockReturnValue(null)
    await fetchJson('/test')
    const call = vi.mocked(fetch).mock.calls[0]
    const opts = call[1] as RequestInit
    expect(
      (opts.headers as Record<string, string>)['Authorization'],
    ).toBeUndefined()
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

  it('throws on 401 and redirects to /login', async () => {
    mockGetToken.mockReturnValue('stale-token')
    const removeItem = vi.fn()
    vi.stubGlobal('localStorage', { removeItem, getItem: vi.fn(), setItem: vi.fn() })
    vi.stubGlobal('window', { ...globalThis.window, location: { href: '' } })
    vi.stubGlobal('fetch', makeFetchMock(401, {}))
    await expect(fetchJson('/secure')).rejects.toThrow('unauthorized')
    expect(removeItem).toHaveBeenCalledWith('mailrs_auth')
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
    expect((opts.headers as Record<string, string>)['Content-Type']).toBe(
      'application/json',
    )
    expect(opts.body).toBe(JSON.stringify(payload))
  })

  it('includes auth header when token exists', async () => {
    mockGetToken.mockReturnValue('my-token')
    vi.stubGlobal('fetch', makeFetchMock(200, {}))
    await postJson('/test', {})
    const opts = vi.mocked(fetch).mock.calls[0][1] as RequestInit
    expect((opts.headers as Record<string, string>)['Authorization']).toBe(
      'Bearer my-token',
    )
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
    const result = await putJson<{ updated: boolean }>('/resource/1', { name: 'test' })
    expect(result).toEqual({ updated: true })
    const opts = vi.mocked(fetch).mock.calls[0][1] as RequestInit
    expect(opts.method).toBe('PUT')
    expect((opts.headers as Record<string, string>)['Content-Type']).toBe(
      'application/json',
    )
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
      status: 200,
      ok: true,
      blob: vi.fn().mockResolvedValue(blob),
    } as unknown as Response)
    vi.stubGlobal('fetch', fetchMock)
    const result = await fetchBlob('/attachment/1')
    expect(result).toBe(blob)
    expect(fetchMock).toHaveBeenCalledWith('/api/attachment/1', expect.any(Object))
  })

  it('throws on 401 and redirects to /login', async () => {
    mockGetToken.mockReturnValue('old-token')
    const removeItem = vi.fn()
    vi.stubGlobal('localStorage', { removeItem, getItem: vi.fn(), setItem: vi.fn() })
    vi.stubGlobal('window', { ...globalThis.window, location: { href: '' } })
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      status: 401,
      ok: false,
      blob: vi.fn(),
    } as unknown as Response))
    await expect(fetchBlob('/attachment/1')).rejects.toThrow('unauthorized')
    expect(removeItem).toHaveBeenCalledWith('mailrs_auth')
    expect(window.location.href).toBe('/login')
  })

  it('throws on non-200 non-401 status', async () => {
    mockGetToken.mockReturnValue(null)
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      status: 404,
      ok: false,
      blob: vi.fn(),
    } as unknown as Response))
    await expect(fetchBlob('/attachment/99')).rejects.toThrow('Download failed: 404')
  })
})
