import { describe, expect, it } from 'vitest'

import { splitEmail, splitHtmlEmail, splitTextEmail } from '../email-split'

describe('splitTextEmail', () => {
  it('returns full text as body when there is no quote', () => {
    const result = splitTextEmail('Hello world')
    expect(result).toEqual({ body: 'Hello world', quoted: null })
  })

  it('returns empty body for empty string', () => {
    const result = splitTextEmail('')
    expect(result).toEqual({ body: '', quoted: null })
  })

  it('keeps a "-- " signature in the body', () => {
    const result = splitTextEmail('Hello\n\n-- \nJohn Doe\njohn@example.com')
    expect(result.body).toContain('John Doe')
    expect(result.body).toContain('john@example.com')
    expect(result.quoted).toBeNull()
  })

  it('keeps a bare "--" signature in the body', () => {
    const result = splitTextEmail('Hello\n\n--\nJohn Doe')
    expect(result.body).toContain('John Doe')
    expect(result.quoted).toBeNull()
  })

  it('extracts quoted text with "On ... wrote:" attribution', () => {
    const text =
      'Thanks!\n\nOn Mon, Jan 1, 2024 at 10:00 AM Alice <alice@example.com> wrote:\n> Original message\n> continues here'
    const result = splitTextEmail(text)
    expect(result.body).toBe('Thanks!')
    expect(result.quoted).toBe(
      'On Mon, Jan 1, 2024 at 10:00 AM Alice <alice@example.com> wrote:\n> Original message\n> continues here'
    )
  })

  it('cuts the quote and keeps the signature above it', () => {
    const text = 'Reply body\n\n-- \nSig line\n\nOn Tue, Feb 2 wrote:\n> quoted line'
    const result = splitTextEmail(text)
    expect(result.body).toContain('Reply body')
    expect(result.body).toContain('Sig line')
    expect(result.quoted).toBe('On Tue, Feb 2 wrote:\n> quoted line')
  })

  it('does not treat "-- " inside quoted text as signature', () => {
    const text = 'My reply\n\nOn Mon wrote:\n> some text\n> -- \n> Their sig'
    const result = splitTextEmail(text)
    expect(result.body).toBe('My reply')
    expect(result.quoted).toBe('On Mon wrote:\n> some text\n> -- \n> Their sig')
  })

  it('handles Outlook "-------- Original Message --------"', () => {
    const text =
      'My reply\n\n-------- Original Message --------\nSubject: Test\nFrom: bob@example.com\n\nOriginal body'
    const result = splitTextEmail(text)
    expect(result.body).toBe('My reply')
    expect(result.quoted).toBe(
      '-------- Original Message --------\nSubject: Test\nFrom: bob@example.com\n\nOriginal body'
    )
  })

  it('handles multi-level quoting', () => {
    const text = 'My reply\n\nOn Mon wrote:\n> First level\n>> Second level\n>>> Third level'
    const result = splitTextEmail(text)
    expect(result.body).toBe('My reply')
    expect(result.quoted).toBe('On Mon wrote:\n> First level\n>> Second level\n>>> Third level')
  })

  it('handles "wrote:" on same line without "On" prefix', () => {
    const text = 'Reply\n\nSomeone wrote:\n> quoted'
    const result = splitTextEmail(text)
    expect(result.body).toBe('Reply')
    expect(result.quoted).toBe('Someone wrote:\n> quoted')
  })

  it('handles quoted block without attribution line', () => {
    const text = 'Reply text\n\n> quoted line 1\n> quoted line 2'
    const result = splitTextEmail(text)
    expect(result.body).toBe('Reply text')
    expect(result.quoted).toBe('> quoted line 1\n> quoted line 2')
  })
})

describe('splitHtmlEmail', () => {
  it('returns full HTML as body when no signature or quote', () => {
    const html = '<div><p>Hello world</p></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Hello world')
    expect(result.quoted).toBeNull()
  })

  it('extracts Gmail signature (.gmail_signature)', () => {
    const html = '<div><p>Body text</p><div class="gmail_signature">-- <br>John</div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Body text')
    expect(result.body).toContain('John')
  })

  it('extracts Gmail quote (.gmail_quote)', () => {
    const html =
      '<div><p>Reply</p><div class="gmail_quote"><blockquote>Original</blockquote></div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Reply')
    expect(result.body).not.toContain('gmail_quote')
    expect(result.quoted).toContain('Original')
  })

  it('cuts the Gmail quote and keeps the signature in the body', () => {
    const html =
      '<div><p>Body</p><div class="gmail_signature">Sig</div><div class="gmail_quote">Quoted</div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Body')
    expect(result.body).toContain('Sig')
    expect(result.quoted).toContain('Quoted')
  })

  it('extracts Outlook quote (#divRplyFwdMsg and siblings)', () => {
    const html =
      '<div><p>Reply</p><div id="divRplyFwdMsg">From: someone</div><div>Original body</div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Reply')
    expect(result.quoted).toContain('From: someone')
    expect(result.quoted).toContain('Original body')
  })

  it('extracts Apple Mail blockquote[type="cite"]', () => {
    const html = '<div><p>My reply</p><blockquote type="cite"><p>Quoted text</p></blockquote></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('My reply')
    expect(result.quoted).toContain('Quoted text')
  })

  it('extracts trailing blockquote as quoted text', () => {
    const html = '<div><p>Reply</p><blockquote><p>Quoted at end</p></blockquote></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Reply')
    expect(result.quoted).toContain('Quoted at end')
  })

  it('does not extract blockquote in middle of content', () => {
    const html = '<div><p>Before</p><blockquote><p>Middle quote</p></blockquote><p>After</p></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Before')
    expect(result.body).toContain('Middle quote')
    expect(result.body).toContain('After')
    expect(result.quoted).toBeNull()
  })

  it('keeps #Signature in the body', () => {
    const html = '<div><p>Body</p><div id="Signature"><p>My Sig</p></div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Body')
    expect(result.body).toContain('My Sig')
  })

  it('extracts #appendonsend and following siblings', () => {
    const html = '<div><p>Reply</p><div id="appendonsend"></div><div>Forwarded content</div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Reply')
    expect(result.quoted).toContain('Forwarded content')
  })

  it('extracts Yahoo quote (.yahoo_quoted)', () => {
    const html = '<div><p>Reply</p><div class="yahoo_quoted">Quoted</div></div>'
    const result = splitHtmlEmail(html)
    expect(result.body).toContain('Reply')
    expect(result.quoted).toContain('Quoted')
  })
})

describe('splitEmail', () => {
  it('prefers HTML when both are available', () => {
    const result = splitEmail(
      'plain text',
      '<div><p>HTML body</p><div class="gmail_quote">Quoted</div></div>'
    )
    expect(result.isHtml).toBe(true)
    expect(result.parts.body).toContain('HTML body')
    expect(result.parts.quoted).toContain('Quoted')
  })

  it('falls back to text when HTML is null', () => {
    const result = splitEmail('Hello\n\n-- \nSig', null)
    expect(result.isHtml).toBe(false)
    expect(result.parts.body).toContain('Hello')
    expect(result.parts.body).toContain('Sig')
  })

  it('returns original on null inputs', () => {
    const result = splitEmail(null, null)
    expect(result.isHtml).toBe(false)
    expect(result.parts.body).toBe('')
    expect(result.parts.quoted).toBeNull()
  })
})
