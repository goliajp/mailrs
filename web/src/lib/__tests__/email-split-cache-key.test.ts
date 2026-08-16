import { describe, expect, it } from 'vitest'

import { splitEmail } from '../email-split'

/**
 * The cache key has to cover both inputs, because the split will soon
 * depend on both.
 *
 * `splitEmail(text, html)` keys on `h:${html}` whenever there is any
 * html at all, so the text part is not in the key. That was harmless
 * while the result depended only on the html — and it stops being
 * harmless the moment a blank-painting html body falls back to the
 * text, which is what the timeline bubble needs (A1 fixed the reading
 * pane and recorded this as the reason the bubble was left alone).
 *
 * Two messages that share a stylesheet-only html body and differ in
 * their text are exactly the shape: the first through the cache would
 * decide what the second displays.
 */
describe('splitEmail cache identity', () => {
  it('does not serve one message the split of another with the same html', () => {
    // Paints nothing: a stylesheet and no text. Identical in both.
    const html = '<html><head><style>.x{color:red}</style></head><body></body></html>'

    const first = splitEmail('the first message body', html)
    const second = splitEmail('a completely different second body', html)

    // Whatever the split decides, it must decide it per message. Sharing
    // a cache entry here is what would make the second show the first.
    expect(second).not.toBe(first)
  })

  it('still caches a genuine repeat', () => {
    const html = '<html><body><p>same</p></body></html>'
    const a = splitEmail('same text', html)
    const b = splitEmail('same text', html)
    expect(b).toBe(a)
  })

  it('keeps text-only messages distinct from each other', () => {
    const a = splitEmail('alpha', null)
    const b = splitEmail('beta', null)
    expect(b).not.toBe(a)
  })
})

/**
 * The bubble's html-vs-text choice, made where the reading pane makes it.
 *
 * A1 fixed the pane: an html body that paints nothing — a stylesheet, a
 * hidden subtree, a remote image with neither size nor alt — is not
 * content, so the pane shows the text instead. The bubble was left on
 * `isHtml`, which only asks whether there is any html at all, and the
 * reason recorded was the cache key above.
 *
 * With the key fixed, `splitEmail` can answer the same question, and it
 * has to answer it the same way: one rule, in `htmlBodyPaintsNothing`,
 * not a second copy that drifts.
 */
describe('splitEmail picks the part that paints something', () => {
  const BLANK_HTML = '<html><head><style>.x{color:red}</style></head><body></body></html>'

  it('falls back to the text when the html paints nothing', () => {
    const { isHtml, parts } = splitEmail('the actual message', BLANK_HTML)
    expect(isHtml).toBe(false)
    expect(parts.body).toContain('the actual message')
  })

  it('keeps the html when it paints something', () => {
    const { isHtml } = splitEmail('plain fallback', '<html><body><p>hello</p></body></html>')
    expect(isHtml).toBe(true)
  })

  it('keeps the html when there is no text to fall back to', () => {
    const { isHtml } = splitEmail(null, BLANK_HTML)
    expect(isHtml).toBe(true)
  })

  it('keeps the html when the text is only whitespace', () => {
    const { isHtml } = splitEmail('   \n  ', BLANK_HTML)
    expect(isHtml).toBe(true)
  })
})
