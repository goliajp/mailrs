import { describe, expect, it } from 'vitest'

import samples from '@/lib/tracker-samples.json'
import { isTrackingImage, stripTrackingParams } from '@/lib/trackers'

function img(attrs: Record<string, string>): HTMLImageElement {
  const el = document.createElement('img')
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
  return el
}

describe('tracking images', () => {
  it('are known tracking services and open-tracking paths', () => {
    expect(isTrackingImage(img({ src: 'https://mailtrack.io/trace/mail/abc.png' }))).toBe(true)
    expect(isTrackingImage(img({ src: 'https://u1.ct.sendgrid.net/wf/open?upn=x' }))).toBe(true)
    expect(isTrackingImage(img({ src: 'https://yesware.com/logo.png' }))).toBe(false)
    expect(isTrackingImage(img({ src: 'https://cdn.shop.test/hero.jpg' }))).toBe(false)
  })

  it('are images drawn a pixel or less, or hidden', () => {
    const src = 'https://cdn.shop.test/p.gif'
    expect(isTrackingImage(img({ height: '1', src, width: '1' }))).toBe(true)
    expect(isTrackingImage(img({ height: '1px', src }))).toBe(true)
    expect(isTrackingImage(img({ src, style: 'width: 1px; height: 1px' }))).toBe(true)
    expect(isTrackingImage(img({ src, style: 'display:none' }))).toBe(true)
    expect(isTrackingImage(img({ height: '1', src, width: '600' }))).toBe(false)
    expect(isTrackingImage(img({ src, width: '100%' }))).toBe(false)
    expect(isTrackingImage(img({ src, style: 'max-width:1px' }))).toBe(false)
  })

  it('never include inline images', () => {
    expect(isTrackingImage(img({ height: '1', src: 'cid:a@b', width: '1' }))).toBe(false)
  })
})

describe('the samples the phones are held to', () => {
  // the same file the iOS and Android tests read, so all three count alike
  it.each(samples.samples)('$name', ({ html, trackers }) => {
    const doc = new DOMParser().parseFromString(html, 'text/html')
    const found = [...doc.querySelectorAll('img')].filter(isTrackingImage).length
    expect(found).toBe(trackers)
  })
})

describe('tracking parameters', () => {
  it('come off a link, the rest of it unchanged', () => {
    expect(
      stripTrackingParams(
        'https://shop.test/sale?id=7&utm_source=mail&UTM_Campaign=x&fbclid=abc#top'
      )
    ).toBe('https://shop.test/sale?id=7#top')
    expect(stripTrackingParams('https://shop.test/a?mc_eid=1')).toBe('https://shop.test/a')
  })

  it('leave a link with none exactly as written', () => {
    const url = 'https://shop.test/a?q=a%20b&id=1'
    expect(stripTrackingParams(url)).toBe(url)
    expect(stripTrackingParams('not a url')).toBe('not a url')
  })
})
