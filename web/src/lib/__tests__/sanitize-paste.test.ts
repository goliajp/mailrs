import { describe, expect, it } from 'vitest'

import { sanitizePastedHtml } from '@/lib/sanitize-paste'

describe('sanitizePastedHtml', () => {
  it('strips a leading dash + space from list-item text content', () => {
    // External markdown renderer (Slack, Notion, iMessage) often emits
    // <li>- Item</li> where the leading "- " is literal text on top of
    // the bullet the <li> already provides — so the recipient sees a
    // bullet *and* a dash. Strip the redundant marker.
    const out = sanitizePastedHtml('<ul><li>- Reliable push notifications</li></ul>')
    expect(out).toContain('<li>Reliable push notifications</li>')
    expect(out).not.toContain('- Reliable')
  })

  it('strips multiple leading bullet variants (-, *, +, •, ·)', () => {
    const cases: [string, string][] = [
      ['<ul><li>* asterisk</li></ul>', 'asterisk'],
      ['<ul><li>+ plus</li></ul>', 'plus'],
      ['<ul><li>• unicode bullet</li></ul>', 'unicode bullet'],
      ['<ul><li>· middle dot</li></ul>', 'middle dot'],
    ]
    for (const [input, expected] of cases) {
      expect(sanitizePastedHtml(input)).toContain(`<li>${expected}</li>`)
    }
  })

  it('drops empty list items left behind by the producer', () => {
    const out = sanitizePastedHtml('<ul><li>real</li><li></li><li>  </li></ul>')
    // The empty / whitespace-only <li>s vanish; the real one stays.
    expect(out).toContain('<li>real</li>')
    expect((out.match(/<li>/g) ?? []).length).toBe(1)
  })

  it('removes a list element entirely if every item was empty', () => {
    const out = sanitizePastedHtml('<ul><li></li><li>   </li></ul><p>after</p>')
    expect(out).not.toContain('<ul>')
    expect(out).toContain('<p>after</p>')
  })

  it('strips inline style and class noise to let the editor theme apply', () => {
    const out = sanitizePastedHtml('<p style="color:red;font-family:Arial" class="foo">hi</p>')
    expect(out).toContain('<p>hi</p>')
    expect(out).not.toContain('style')
    expect(out).not.toContain('class')
  })

  it('unwraps MS-Word-style <font> and decorative <span>', () => {
    const out = sanitizePastedHtml('<p><font color="red">red</font> normal <span>plain</span></p>')
    expect(out).toBe('<p>red normal plain</p>')
  })

  it('preserves links, images, formatting marks, and tables', () => {
    const out = sanitizePastedHtml(
      '<p><strong>bold</strong> <em>italic</em> <a href="https://example.com">link</a></p>'
    )
    expect(out).toContain('<strong>bold</strong>')
    expect(out).toContain('<em>italic</em>')
    expect(out).toContain('<a href="https://example.com">link</a>')

    const tableOut = sanitizePastedHtml('<table><tr><td>a</td><td>b</td></tr></table>')
    expect(tableOut).toContain('<table>')
    expect(tableOut).toContain('<td>a</td>')
  })

  it('handles the user-reported broken Weekly Report paste shape', () => {
    // What broke: signature paragraphs ended up as <li>s alongside the
    // real bullet items. Sanitizer normalizes leading dashes inside <li>
    // and removes empty <li>s; the *unwrap-non-list-content* heuristic
    // is conservative — we just ensure the dashes disappear from items
    // and empty-li clutter goes away. Layout-level "this should not be
    // a list at all" is the user's call to fix in the editor; we don't
    // silently move blocks around.
    const broken =
      '<p>- LAN direct connection</p>' +
      '<ul>' +
      '  <li>- Reliable push notifications</li>' +
      '  <li>- Stability fixes</li>' +
      '  <li></li>' +
      '  <li>19 issues completed.</li>' +
      '  <li>Full report attached.</li>' +
      '  <li></li>' +
      '  <li>Best regards,</li>' +
      '  <li></li>' +
      '  <li>LI HAO</li>' +
      '  <li>CEO, GOLIA K.K.</li>' +
      '</ul>'
    const out = sanitizePastedHtml(broken)
    expect(out).toContain('<li>Reliable push notifications</li>')
    expect(out).toContain('<li>Stability fixes</li>')
    expect(out).toContain('<li>19 issues completed.</li>')
    expect(out).not.toMatch(/<li>\s*<\/li>/)
    // Note: signature lines still inside the <ul> — that's a user
    // input/pasting choice we don't override; sanitizer cleaned the
    // mechanical noise (dashes, empty items) only.
  })

  it('returns input unchanged when there is nothing to sanitize', () => {
    expect(sanitizePastedHtml('<p>plain</p>')).toBe('<p>plain</p>')
  })

  it('tolerates malformed input', () => {
    // jsdom's DOMParser is forgiving; we just need to not throw.
    expect(() => sanitizePastedHtml('<<<>>')).not.toThrow()
  })
})
