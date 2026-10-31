import type { RemoteImages } from '@/store/settings'

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { createHash } from 'node:crypto'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

import { markRemoteImage } from '@/lib/remote-images'
import { imageSendersAtom, remoteImagesAtom } from '@/store/settings'
import { wirePrefetchThreadImages } from '@/wire/endpoints/mail'

import { HtmlFrame } from '../html-frame'

vi.mock('@/wire/endpoints/mail', () => ({ wirePrefetchThreadImages: vi.fn() }))

const REMOTE = 'https://tracker.example/open.gif?u=1&m=2'

function sha256(text: string): string {
  return createHash('sha256').update(text).digest('hex')
}

beforeAll(() => {
  // jsdom has none; the frame only measures with it
  vi.stubGlobal(
    'ResizeObserver',
    class {
      disconnect() {}
      observe() {}
      unobserve() {}
    }
  )
})

describe('remote images in mail', () => {
  it('leave no remote address on the image put into the page', () => {
    const img = document.createElement('img')
    img.setAttribute('src', ' https://tracker.example/a.gif?x=1&y=2 ')
    markRemoteImage(img)
    expect(img.hasAttribute('src')).toBe(false)
    expect(img.getAttribute('data-remote-src')).toBe('https://tracker.example/a.gif?x=1&y=2')
    const inline = document.createElement('img')
    inline.setAttribute('src', 'cid:part@x')
    markRemoteImage(inline)
    expect(inline.getAttribute('src')).toBe('cid:part@x')
  })

  it("load the prefetched copy, never the sender's server", async () => {
    const html = `<p>hi</p><img src="https://tracker.example/open.gif?u=1&amp;m=2" alt="logo"><img src="cid:inline@x">`
    const { container } = render(<HtmlFrame attachments={[]} html={html} uid={1} />)
    const host = container.querySelector<HTMLElement>('div')
    const root = host?.shadowRoot ?? container.querySelector('*')?.shadowRoot
    expect(root).toBeTruthy()
    const remote = () => root!.querySelector<HTMLImageElement>('img[alt="logo"]')!
    await vi.waitFor(() =>
      expect(remote().getAttribute('src')).toBe(`/api/mail/image/${sha256(REMOTE)}`)
    )
    for (const img of root!.querySelectorAll('img')) {
      expect(img.getAttribute('src') ?? '').not.toContain('tracker.example')
    }
  })

  // mail from before images were fetched on arrival: the miss asks the
  // server for them once, and the image is asked for again later
  it('a missing image is fetched on request and loaded again', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    vi.mocked(wirePrefetchThreadImages).mockResolvedValue('queued')
    const html = '<img src="https://old.example/banner.png" alt="old">'
    const { container } = render(<HtmlFrame html={html} imagesOf="t-1" />)
    const root = container.querySelector<HTMLElement>('div')!.shadowRoot!
    const img = () => root.querySelector<HTMLImageElement>('img[alt="old"]')!
    await vi.waitFor(() => expect(img().getAttribute('src')).toContain('/api/mail/image/'))
    img().dispatchEvent(new Event('error'))
    await vi.advanceTimersByTimeAsync(600)
    expect(wirePrefetchThreadImages).toHaveBeenCalledWith('t-1')
    await vi.advanceTimersByTimeAsync(4_000)
    expect(img().getAttribute('src')).toContain('?attempt=1')
    vi.useRealTimers()
  })

  it('says why the images of mail in Junk are not shown', async () => {
    vi.mocked(wirePrefetchThreadImages).mockResolvedValue('junk')
    const html = '<img src="https://spam.example/pixel.gif" alt="px">'
    const { container } = render(<HtmlFrame html={html} imagesOf="t-junk" />)
    const root = container.querySelector<HTMLElement>('div:last-child')!.shadowRoot!
    const img = root.querySelector<HTMLImageElement>('img[alt="px"]')!
    await vi.waitFor(() => expect(img.getAttribute('src')).toContain('/api/mail/image/'))
    img.dispatchEvent(new Event('error'))
    expect(await screen.findByText(/not loaded for mail in Junk/)).toBeDefined()
  })
})

describe('the remote images setting', () => {
  afterEach(() => {
    cleanup()
    localStorage.clear()
  })

  function renderWith(mode: RemoteImages, html: string, imagesOf?: string) {
    const store = createStore()
    store.set(remoteImagesAtom, mode)
    return render(
      <Provider store={store}>
        <HtmlFrame html={html} imagesOf={imagesOf} />
      </Provider>
    )
  }

  it('on click: images wait until the reader asks', async () => {
    const { container } = renderWith('click', '<img src="https://a.example/x.png" alt="x">')
    const root = container.querySelector<HTMLElement>('div:last-child')!.shadowRoot!
    const img = () => root.querySelector<HTMLImageElement>('img[alt="x"]')!
    await new Promise((r) => setTimeout(r, 50))
    expect(img().hasAttribute('src')).toBe(false)
    fireEvent.click(screen.getByText('Show images'))
    await vi.waitFor(() => expect(img().getAttribute('src')).toContain('/api/mail/image/'))
  })

  it('never: images stay hidden and nothing offers to show them', async () => {
    const { container } = renderWith('never', '<img src="https://a.example/y.png" alt="y">')
    const root = container.querySelector<HTMLElement>('div:last-child')!.shadowRoot!
    await new Promise((r) => setTimeout(r, 50))
    expect(root.querySelector('img[alt="y"]')!.hasAttribute('src')).toBe(false)
    expect(screen.getByText('Images are hidden by your settings')).toBeDefined()
    expect(screen.queryByText('Show images')).toBeNull()
  })

  it('a message without remote images says nothing', () => {
    renderWith('click', '<p>plain</p>')
    expect(screen.queryByText(/hidden/)).toBeNull()
  })

  it('Junk mail: showing anyway asks the server past the Junk refusal', async () => {
    vi.mocked(wirePrefetchThreadImages).mockReset()
    vi.mocked(wirePrefetchThreadImages).mockResolvedValue('junk')
    const { container } = renderWith('auto', '<img src="https://s.example/p.gif" alt="p">', 't-j')
    const root = container.querySelector<HTMLElement>('div:last-child')!.shadowRoot!
    const img = root.querySelector<HTMLImageElement>('img[alt="p"]')!
    await vi.waitFor(() => expect(img.getAttribute('src')).toContain('/api/mail/image/'))
    img.dispatchEvent(new Event('error'))
    fireEvent.click(await screen.findByText('Show images anyway'))
    expect(wirePrefetchThreadImages).toHaveBeenCalledWith('t-j', true)
  })
})

describe('senders whose images always show', () => {
  afterEach(() => {
    cleanup()
    localStorage.clear()
  })

  function frame(store: ReturnType<typeof createStore>, from?: string) {
    return render(
      <Provider store={store}>
        <HtmlFrame html='<img src="https://news.example/x.png" alt="n">' imagesFrom={from} />
      </Provider>
    )
  }

  it('a remembered, authenticated sender shows images under On click', async () => {
    const store = createStore()
    store.set(remoteImagesAtom, 'click')
    store.set(imageSendersAtom, 'News@Shop.example')
    const { container } = frame(store, 'news@shop.example')
    const root = container.querySelector<HTMLElement>('div:last-child')!.shadowRoot!
    await vi.waitFor(() =>
      expect(root.querySelector('img[alt="n"]')!.getAttribute('src')).toContain('/api/mail/image/')
    )
  })

  it('the reader can remember the sender, and nothing is offered without one', async () => {
    const store = createStore()
    store.set(remoteImagesAtom, 'click')
    frame(store, 'deals@shop.example')
    fireEvent.click(screen.getByText('Always show from deals@shop.example'))
    expect(store.get(imageSendersAtom)).toContain('deals@shop.example')
    expect(JSON.parse(localStorage.getItem('mailrs_image_senders') ?? '[]')).toContain(
      'deals@shop.example'
    )
    cleanup()
    frame(store)
    expect(screen.queryByText(/Always show from/)).toBeNull()
  })
})
