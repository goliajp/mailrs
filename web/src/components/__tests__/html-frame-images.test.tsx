import { render } from '@testing-library/react'
import { createHash } from 'node:crypto'
import { beforeAll, describe, expect, it, vi } from 'vitest'

import { markRemoteImage } from '@/lib/remote-images'

import { HtmlFrame } from '../html-frame'

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
})
