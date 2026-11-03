import { cleanup, render } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

import { chosenLangAtom } from '@/i18n'
import { type LinkWarning, markLinkWarnings, warningFor } from '@/lib/link-warnings'

import { HtmlFrame } from '../html-frame'

vi.mock('@/wire/endpoints/mail', () => ({ wirePrefetchThreadImages: vi.fn() }))

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

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const BODY =
  '<p><a href="https://webmail-oauth.wzldq.com/auth">www. golia.jp /lihao/check</a> ' +
  '<a href="https://bad.example/x">here</a> ' +
  '<a href="https://webmail-oauth.wzldq.com/other">other text</a> ' +
  '<a href="https://fine.example/">fine</a></p>'

const WARNINGS: LinkWarning[] = [
  {
    host: 'webmail-oauth.wzldq.com',
    reasons: [
      {
        code: 'link-shows-another-host',
        params: { host: 'webmail-oauth.wzldq.com', shown: 'www.golia.jp' },
      },
    ],
    shown: 'www.golia.jp',
  },
  { host: 'bad.example', reasons: [{ code: 'link-host-listed', params: { host: 'bad.example' } }] },
]

function frame(lang: 'en' | 'zh') {
  const store = createStore()
  store.set(chosenLangAtom, lang)
  const view = render(
    <Provider store={store}>
      <HtmlFrame html={BODY} linkWarnings={WARNINGS} />
    </Provider>
  )
  const root = view.container.firstElementChild?.shadowRoot
  if (!root) throw new Error('no shadow root')
  return root
}

function marked(root: ShadowRoot): string[] {
  return [...root.querySelectorAll('a[data-link-warning]')].map((a) => a.textContent ?? '')
}

describe('links the server flagged', () => {
  it('are marked where they appear, and only those', () => {
    const root = frame('en')
    // the second link to the same host shows no other address, so it is not the claim
    expect(marked(root)).toEqual(['www. golia.jp /lihao/check', 'here'])
    const mark = root.querySelector('.link-warning')
    expect(mark?.getAttribute('title')).toBe(
      'This link shows www.golia.jp but goes to webmail-oauth.wzldq.com.'
    )
  })

  it('give their reason in the reader’s language', () => {
    const root = frame('zh')
    const titles = [...root.querySelectorAll('.link-warning')].map((m) => m.getAttribute('title'))
    expect(titles[1]).toBe('这个链接指向 bad.example，它在诈骗主机名单上。')
  })

  it('say when a link goes to a domain first seen this week', () => {
    const store = createStore()
    store.set(chosenLangAtom, 'zh')
    const view = render(
      <Provider store={store}>
        <HtmlFrame
          html={BODY}
          linkWarnings={[
            {
              host: 'fine.example',
              reasons: [{ code: 'link-host-new', params: { host: 'fine.example' } }],
            },
          ]}
        />
      </Provider>
    )
    const root = view.container.firstElementChild?.shadowRoot
    expect(root?.querySelector('.link-warning')?.getAttribute('title')).toBe(
      '这个链接指向 fine.example，这个域名本周才第一次见到。'
    )
  })

  it('ask before they open, and stay shut when the reader says no', () => {
    const root = frame('en')
    const ask = vi.spyOn(window, 'confirm').mockReturnValue(false)
    const a = root.querySelector<HTMLAnchorElement>('a[data-link-warning]')
    const click = new MouseEvent('click', { bubbles: true, cancelable: true })
    a?.dispatchEvent(click)
    expect(ask).toHaveBeenCalledOnce()
    expect(click.defaultPrevented).toBe(true)
  })

  it('show the reason on a tap of the mark', () => {
    const root = frame('en')
    const note = root.querySelector<HTMLElement>('.link-warning-note')
    expect(note?.hidden).toBe(true)
    root.querySelector<HTMLElement>('.link-warning')?.click()
    expect(note?.hidden).toBe(false)
  })
})

describe('matching an anchor to a warning', () => {
  it('reads the target back from the proxied address', () => {
    const a = document.createElement('a')
    a.href = `/api/proxy/link?url=${encodeURIComponent('https://Bad.Example/x?y=1')}`
    a.textContent = 'here'
    expect(warningFor(a, WARNINGS)?.host).toBe('bad.example')
  })

  it('marks nothing when there is nothing to warn about', () => {
    const div = document.createElement('div')
    div.innerHTML = BODY
    expect(
      markLinkWarnings(
        div,
        [],
        () => '',
        () => true
      )
    ).toBe(0)
  })
})
