import { describe, expect, it } from 'vitest'

import { makeMessage } from '@/components/__tests__/thread-view-fixtures'
import { previewText } from '@/lib/message-preview'

const html = (body: string) =>
  makeMessage({
    clean_text: null,
    html_body: body,
    new_content: null,
    summary: '',
    text_body: null,
  })

describe('previewText of an html-only message', () => {
  it('keeps the words when the footer is a row of links split by bars', () => {
    const m = html(
      '<p>Hello team, the invoice is attached for review.</p><p>Home | Privacy | Terms | Help | Unsubscribe</p>'
    )
    expect(previewText(m, '')).toMatch(/^Hello team, the invoice is attached for review\. Home/)
  })

  it('keeps the words when the mail is full of bracketed links and braces', () => {
    const m = html(
      '<table><tr><td>Invoice</td><td>{amount}</td></tr></table><p>Please pay [now] {soon} <b>x</b></p>'
    )
    expect(previewText(m, '')).toBe('Invoice {amount} Please pay [now] {soon} x')
  })

  it('leaves out style, script and the quoted earlier mail', () => {
    const m = html(
      '<html><head><style>.a{color:red}</style></head><body><style>p{margin:0}</style>' +
        '<script>var x = 1</script><div>Thanks, works for me.</div>' +
        '<div class="gmail_quote"><div class="gmail_attr">On Mon, A wrote:</div>' +
        '<blockquote class="gmail_quote">the old message</blockquote></div></body></html>'
    )
    expect(previewText(m, '')).toBe('Thanks, works for me.')
  })

  it('leaves out an Apple Mail quote and everything after an Outlook reply header', () => {
    expect(
      previewText(html('<div>Sounds good</div><blockquote type="cite">old</blockquote>'), '')
    ).toBe('Sounds good')
    const outlook = html(
      '<div><p>See you then</p></div><div id="appendonsend"></div><hr>' +
        '<div id="divRplyFwdMsg"><b>From:</b> A</div><div>the old message</div>'
    )
    expect(previewText(outlook, '')).toBe('See you then')
  })

  it('folds whitespace to one space and puts one between blocks', () => {
    const m = html('<div>first\n\n   line</div><div>second</div><p>third<br>fourth</p>')
    expect(previewText(m, '')).toBe('first line second third fourth')
  })

  it('is cut to the length of a plain-text preview', () => {
    const words = 'word '.repeat(80).trim()
    const fromHtml = previewText(html(`<p>${words}</p>`), '')
    const fromText = previewText(
      makeMessage({ html_body: null, summary: '', text_body: words }),
      ''
    )
    expect(fromHtml).toBe(fromText)
    expect(fromHtml.length).toBeLessThanOrEqual(201)
  })
})
