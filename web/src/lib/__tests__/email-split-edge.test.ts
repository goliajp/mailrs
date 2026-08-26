import { describe, expect, it } from 'vitest'

import { splitEmail, splitHtmlEmail, splitTextEmail } from '../email-split'

describe('splitTextEmail edge cases', () => {
  it('handles only whitespace', () => {
    const result = splitTextEmail('   \n  \n  ')
    expect(result.body).toBe('')
    expect(result.quoted).toBeNull()
  })

  it('keeps a trailing "-- " marker in the body', () => {
    const result = splitTextEmail('Hello\n\n-- ')
    expect(result.body).toBe('Hello\n\n--')
  })

  it('keeps every signature block in the body', () => {
    // They used to be cut here and rendered nowhere, which meant the
    // sender's name simply vanished.
    const result = splitTextEmail('Hello\n\n-- \nFirst sig\n\n-- \nSecond sig')
    expect(result.body).toContain('First sig')
    expect(result.body).toContain('Second sig')
  })

  it('handles text that is only quoted lines', () => {
    const result = splitTextEmail('> quoted line 1\n> quoted line 2')
    expect(result.body).toBe('')
    expect(result.quoted).toBe('> quoted line 1\n> quoted line 2')
  })

  it('handles quoted text with blank lines between', () => {
    const result = splitTextEmail('Reply\n\n> line 1\n\n> line 2')
    expect(result.body).toBe('Reply')
    expect(result.quoted).toContain('> line 1')
    expect(result.quoted).toContain('> line 2')
  })

  it('handles Windows-style line endings in attribution', () => {
    const text = 'Reply\r\n\r\nSomeone wrote:\r\n> quoted'
    const result = splitTextEmail(text)
    // \r will be kept in the split since we split on \n
    expect(result.body.trim()).toBe('Reply')
  })

  it('handles very long attribution line', () => {
    const longAttribution = 'On ' + 'X'.repeat(150) + ' wrote:'
    const text = `Reply\n\n${longAttribution}\n> quoted text`
    const result = splitTextEmail(text)
    expect(result.body).toBe('Reply')
    expect(result.quoted).toContain('wrote:')
  })

  it('ignores attribution line exceeding 200 chars', () => {
    const longAttribution = 'On ' + 'X'.repeat(250) + ' wrote:'
    const text = `Reply\n\n${longAttribution}\n> quoted text`
    const result = splitTextEmail(text)
    // line too long for ATTRIBUTION_RE (max 200 chars)
    // but the trailing > block fallback should still catch the quotes
    expect(result.quoted).toContain('> quoted text')
  })

  it('handles text with only "-- " marker', () => {
    const result = splitTextEmail('-- ')
    expect(result.body).toBe('--')
  })
})

describe('splitHtmlEmail edge cases', () => {
  it('handles empty HTML', () => {
    const result = splitHtmlEmail('')
    expect(result.body).toBe('')
    expect(result.quoted).toBeNull()
  })

  it('handles plain text in HTML wrapper', () => {
    const result = splitHtmlEmail('<html><body>Plain text</body></html>')
    expect(result.body).toContain('Plain text')
  })

  it('keeps gmail_signature blocks in the body', () => {
    const html =
      '<div><p>Body</p><div class="gmail_signature">Sig1</div><div class="gmail_signature">Sig2</div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Sig1')
    expect(result.body).toContain('Sig2')
  })

  it('handles nested blockquotes in gmail_quote', () => {
    const html =
      '<div><p>Reply</p><div class="gmail_quote"><blockquote><blockquote>Nested</blockquote></blockquote></div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Reply')
    expect(result.quoted).toContain('Nested')
  })

  it('keeps #Signature and #signature blocks in the body', () => {
    // Outlook Web wraps whatever is in the composing area in
    // `id="Signature"`, so this is not reliably a signature at all.
    const html1 = '<div><p>Body</p><div id="Signature">Upper</div></div>'
    const html2 = '<div><p>Body</p><div id="signature">Lower</div></div>'
    expect(splitHtmlEmail(html1).body).toContain('Upper')
    expect(splitHtmlEmail(html2).body).toContain('Lower')
  })

  it('handles malformed HTML without crashing', () => {
    const result = splitHtmlEmail('<div><p>Unclosed paragraph<div>Another')
    expect(typeof result.body).toBe('string')
  })

  it('handles HTML with only a blockquote', () => {
    const html = '<blockquote>Only quoted</blockquote>'
    const result = splitHtmlEmail(html)
    expect(result.quoted).toContain('Only quoted')
    expect(result.body).toBe('')
  })
})

describe('splitEmail edge cases', () => {
  it('handles empty string text with empty string HTML', () => {
    const result = splitEmail('', '')
    // empty string is falsy, so falls back to text
    expect(result.isHtml).toBe(false)
    expect(result.parts.body).toBe('')
  })

  it('prefers HTML over text when HTML has content', () => {
    const result = splitEmail('text body', '<p>html body</p>')
    expect(result.isHtml).toBe(true)
    expect(result.parts.body).toContain('html body')
  })

  it('uses text when HTML is empty string', () => {
    const result = splitEmail('text body', '')
    expect(result.isHtml).toBe(false)
    expect(result.parts.body).toBe('text body')
  })

  it('handles both null inputs', () => {
    const result = splitEmail(null, null)
    expect(result.isHtml).toBe(false)
    expect(result.parts.body).toBe('')
    expect(result.parts.quoted).toBeNull()
  })

  it('handles text null with valid HTML', () => {
    const result = splitEmail(null, '<p>HTML only</p>')
    expect(result.isHtml).toBe(true)
    expect(result.parts.body).toContain('HTML only')
  })
})
