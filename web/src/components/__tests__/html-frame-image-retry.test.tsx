import type { RemoteImages } from '@/store/settings'

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import { remoteImagesAtom } from '@/store/settings'
import { wirePrefetchThreadImages } from '@/wire/endpoints/mail'

import { HtmlFrame } from '../html-frame'

vi.mock('@/wire/endpoints/mail', () => ({ wirePrefetchThreadImages: vi.fn() }))

// The notes change on a real macrotask (React renders a change made outside
// an event that way), which moving the fake clock does not run: each check
// of a note waits for it.

type Outcome = 'error' | 'load'

function frame(html: string, mode: RemoteImages = 'auto') {
  const store = createStore()
  store.set(remoteImagesAtom, mode)
  const view = render(
    <Provider store={store}>
      <HtmlFrame html={html} imagesOf="t-1" />
    </Provider>
  )
  const root = view.container.querySelector<HTMLElement>('div[class*="isolate"]')!.shadowRoot!
  return { root, view }
}

/**
 * Stands in for `GET /api/mail/image/{hash}`, which jsdom never asks: each
 * time an image gets an address it fails, until the server has the copy
 * (`readyAt`, by the fake clock; `null` for never), and then it loads.
 */
function serveImages(root: ShadowRoot, readyAt: (alt: string) => null | number) {
  const last = new Map<string, Outcome>()
  const srcs = new Map<string, string[]>()
  const observer = new MutationObserver((records) => {
    for (const r of records) {
      const img = r.target as HTMLImageElement
      const alt = img.alt
      srcs.set(alt, [...(srcs.get(alt) ?? []), img.getAttribute('src') ?? ''])
      const at = readyAt(alt)
      let outcome: Outcome = 'error'
      if (at !== null && Date.now() >= at) outcome = 'load'
      last.set(alt, outcome)
      img.dispatchEvent(new Event(outcome))
    }
  })
  observer.observe(root, { attributeFilter: ['src'], attributes: true, subtree: true })
  return { last, srcs, stop: () => observer.disconnect() }
}

const ONE = '<img src="https://news.example/banner.png" alt="banner">'
const TWO =
  '<img src="https://news.example/hero.png" alt="hero"><img src="https://news.example/late.png" alt="late">'

beforeAll(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      disconnect() {}
      observe() {}
      unobserve() {}
    }
  )
})

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true })
  vi.mocked(wirePrefetchThreadImages).mockReset()
  vi.mocked(wirePrefetchThreadImages).mockResolvedValue('queued')
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  localStorage.clear()
})

describe('images the server fetches while the reader waits', () => {
  it('an image that takes longer than 30 seconds still shows, without a reload', async () => {
    const start = Date.now()
    const { root } = frame(ONE)
    const served = serveImages(root, () => start + 40_000)
    await vi.waitFor(() => expect(served.last.get('banner')).toBe('error'))
    await vi.advanceTimersByTimeAsync(600)
    expect(wirePrefetchThreadImages).toHaveBeenCalledWith('t-1')
    await vi.advanceTimersByTimeAsync(130_000)
    expect(served.last.get('banner')).toBe('load')
    await vi.waitFor(() => expect(screen.queryByText('Loading images')).toBeNull())
    await vi.waitFor(() => expect(screen.queryByText('Some images could not be loaded')).toBeNull())
    served.stop()
  })

  it('says so plainly when images are still missing after the last attempt', async () => {
    const { root } = frame(ONE)
    const served = serveImages(root, () => null)
    await vi.waitFor(() => expect(served.last.get('banner')).toBe('error'))
    await vi.advanceTimersByTimeAsync(60_000)
    await vi.waitFor(() => expect(screen.getByText('Loading images')).toBeDefined())
    await vi.advanceTimersByTimeAsync(70_000)
    await vi.waitFor(() =>
      expect(screen.getByText('Some images could not be loaded')).toBeDefined()
    )
    await vi.waitFor(() => expect(screen.queryByText('Loading images')).toBeNull())
    const asks = served.srcs.get('banner')!.length
    await vi.advanceTimersByTimeAsync(300_000)
    expect(served.srcs.get('banner')!.length).toBe(asks)
    served.stop()
  })

  it('an image that loaded is not asked for again', async () => {
    const start = Date.now()
    const { root } = frame(TWO)
    const served = serveImages(root, (alt) => {
      if (alt === 'hero') return start
      return start + 20_000
    })
    await vi.waitFor(() => expect(served.last.get('late')).toBe('error'))
    await vi.advanceTimersByTimeAsync(130_000)
    expect(served.last.get('late')).toBe('load')
    expect(served.srcs.get('hero')).toHaveLength(1)
    expect(served.srcs.get('hero')![0]).not.toContain('?')
    served.stop()
  })

  it('stops asking once the message is closed', async () => {
    const { root, view } = frame(ONE)
    const served = serveImages(root, () => null)
    await vi.waitFor(() => expect(served.last.get('banner')).toBe('error'))
    await vi.advanceTimersByTimeAsync(3_000)
    const asks = served.srcs.get('banner')!.length
    view.unmount()
    await vi.advanceTimersByTimeAsync(300_000)
    expect(served.srcs.get('banner')!.length).toBe(asks)
    served.stop()
  })

  it('Junk mail: "show anyway" loads images that take longer than 30 seconds', async () => {
    vi.mocked(wirePrefetchThreadImages).mockImplementation(async (_tid, anyway) => {
      if (anyway) return 'queued'
      return 'junk'
    })
    const start = Date.now()
    const { root } = frame(ONE)
    const served = serveImages(root, () => start + 45_000)
    await vi.waitFor(() => expect(served.last.get('banner')).toBe('error'))
    fireEvent.click(await screen.findByText('Show images anyway'))
    expect(wirePrefetchThreadImages).toHaveBeenCalledWith('t-1', true)
    await vi.advanceTimersByTimeAsync(130_000)
    expect(served.last.get('banner')).toBe('load')
    await vi.waitFor(() => expect(screen.queryByText('Loading images')).toBeNull())
    served.stop()
  })

  it('Junk mail: closing the message after "show anyway" leaves no timer behind', async () => {
    vi.mocked(wirePrefetchThreadImages).mockImplementation(async (_tid, anyway) => {
      if (anyway) return 'queued'
      return 'junk'
    })
    const { root, view } = frame(ONE)
    const served = serveImages(root, () => null)
    await vi.waitFor(() => expect(served.last.get('banner')).toBe('error'))
    fireEvent.click(await screen.findByText('Show images anyway'))
    await vi.advanceTimersByTimeAsync(100)
    const asks = served.srcs.get('banner')!.length
    view.unmount()
    await vi.advanceTimersByTimeAsync(300_000)
    expect(served.srcs.get('banner')!.length).toBe(asks)
    served.stop()
  })
})
