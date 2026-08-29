import { render, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SenderAvatar } from '../sender-avatar'

/**
 * SenderAvatar has to survive the two shapes the wire can be in:
 *   1. `/api/icon/<domain>` returns 200 + real bytes → render <img>
 *   2. `/api/icon/<domain>` returns 204 No Content   → render initial
 *
 * The tests also nail the auth-token gate — when no token is available
 * (logged-out preview / SSR) the component must NOT fire an anonymous
 * fetch, because that regresses the whole 401-spam issue we just fixed.
 */

// The auth store's `getToken()` parses this JSON from localStorage;
// see `src/store/auth.ts::STORAGE_KEY`.
const AUTH_JSON = JSON.stringify({ address: 'a@b.c', token: 'test-token' })

function stubAuthStorage(token: null | string) {
  vi.stubGlobal('localStorage', {
    length: 0,
    clear: () => undefined,
    getItem: (k: string) =>
      k === 'mailrs_auth' && token != null ? JSON.stringify({ address: 'a@b.c', token }) : null,
    key: () => null,
    removeItem: () => undefined,
    setItem: () => undefined,
  })
}

beforeEach(() => {
  stubAuthStorage('test-token')
  // URL.createObjectURL isn't in jsdom.
  Object.defineProperty(globalThis.URL, 'createObjectURL', {
    configurable: true,
    value: (blob: Blob) => `blob:${blob.size}`,
  })
  // Ping AUTH_JSON so the linter doesn't flag it while we keep it
  // as a self-documenting fixture.
  void AUTH_JSON
})

describe('<SenderAvatar />', () => {
  it('renders the letter avatar when the icon endpoint returns 204 No Content', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)

    const { container } = render(<SenderAvatar sender="Alice <alice@nofavicon-1.example>" />)
    // Wait one microtask so the resolveIcon promise settles.
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    // No <img> — the initial letter avatar is the fallback.
    expect(container.querySelector('img')).toBeNull()
    expect(container.textContent).toBe('A')
  })

  it('renders an <img> when the icon endpoint returns 200 + image bytes', async () => {
    const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47]) // PNG magic
    const fetchMock = vi.fn(
      async () =>
        new Response(bytes, {
          headers: { 'content-type': 'image/png' },
          status: 200,
        })
    )
    vi.stubGlobal('fetch', fetchMock)

    const { container } = render(<SenderAvatar sender="LinkedIn <notify@linked-in-2.example>" />)
    await waitFor(() => expect(container.querySelector('img')).not.toBeNull())
    const img = container.querySelector('img')!
    expect(img.src).toMatch(/^blob:/)
  })

  it('never fires an anonymous fetch when no auth token is present', async () => {
    stubAuthStorage(null)
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const { container } = render(<SenderAvatar sender="Bob <bob@no-auth-3.example>" />)
    // Give the effect a tick to (not) fire.
    await new Promise((r) => setTimeout(r, 5))
    expect(fetchMock).not.toHaveBeenCalled()
    expect(container.textContent).toBe('B')
  })

  it('passes the Bearer token when auth is available', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)

    render(<SenderAvatar sender="Carol <carol@auth-4.example>" />)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    const call = fetchMock.mock.calls[0] as unknown as [string, { headers: Record<string, string> }]
    expect(call[1]?.headers).toEqual({ Authorization: 'Bearer test-token' })
  })

  /**
   * The reading pane passes an **empty sender on purpose** when the
   * message is a suspected spoof, so the phish is not drawn wearing a
   * real brand's logo. The effect returned early on an empty domain
   * without clearing, so a reused component kept the icon of whatever
   * was read before it — an "iCloud+" phish from `zkxfp.zctxiot.com`
   * rendered with TikTok's mark, which is the exact opposite of what
   * the empty sender was for.
   */
  it('drops the previous logo when the next sender has no domain', async () => {
    const png = new Uint8Array([137, 80, 78, 71])
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () => new Response(png, { headers: { 'content-type': 'image/png' }, status: 200 })
      )
    )

    const { container, rerender } = render(<SenderAvatar sender="TikTok <a@brand-1.example>" />)
    await waitFor(() => expect(container.querySelector('img')).not.toBeNull())

    // The spoof: no sender at all.
    rerender(<SenderAvatar sender="" />)
    await waitFor(() => expect(container.querySelector('img')).toBeNull())
  })

  /** And it does not show the old logo while the new one is loading. */
  it('drops the previous logo while the next sender is still resolving', async () => {
    const png = new Uint8Array([137, 80, 78, 71])
    let release: (() => void) | null = null
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (String(url).includes('brand-2')) {
          return new Response(png, { headers: { 'content-type': 'image/png' }, status: 200 })
        }
        await new Promise<void>((r) => {
          release = r
        })
        return new Response(null, { status: 204 })
      })
    )

    const { container, rerender } = render(<SenderAvatar sender="A <a@brand-2.example>" />)
    await waitFor(() => expect(container.querySelector('img')).not.toBeNull())

    rerender(<SenderAvatar sender="B <b@slow-3.example>" />)
    await waitFor(() => expect(container.querySelector('img')).toBeNull())
    release?.()
  })
})
