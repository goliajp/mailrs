import { describe, expect, it } from 'vitest'

import { splitHtmlEmail } from '@/lib/email-split'

/**
 * Outlook Web wraps whatever sits in the composing area in a div with
 * `id="Signature"` — the sender's whole message can be inside it.
 *
 * Shape taken from a message that arrived on 2026-08-26: the reader saw
 * "Hello Li," and a horizontal rule, and nothing else. The three
 * sentences that were the actual mail were in the div below the
 * greeting, and the splitter deleted them as a signature — into a field
 * no view anywhere rendered, so extracting it meant discarding it.
 *
 * The preview beside it, which flattens the raw html, showed the
 * sentence the pane was hiding. That is the tell: two renderings of one
 * message disagreeing about whether it has any content.
 */
describe('an Outlook Web message whose body is inside #Signature', () => {
  const html = [
    '<html><body>',
    '<div class="elementToProof">Hello Li,</div>',
    '<div id="Signature" class="elementToProof">',
    '<div id="x_x_x_Signature" class="elementToProof">',
    'I trust you are doing well! Do you have any updates regarding your',
    ' participation? Please review the proposal below and share your feedback.',
    '</div>',
    '<div class="elementToProof">Best Regards,</div>',
    '<div class="elementToProof">Maria Carter</div>',
    '</div>',
    '<div id="appendonsend"></div>',
    '<hr>',
    '<div id="divRplyFwdMsg">From: Maria Carter</div>',
    '<div>the earlier message</div>',
    '</body></html>',
  ].join('')

  it('keeps the sentences the reader is meant to read', () => {
    const parts = splitHtmlEmail(html)
    expect(parts.body).toContain('Hello Li,')
    expect(parts.body).toContain('I trust you are doing well')
    expect(parts.body).toContain('Please review the proposal')
    expect(parts.body).toContain('Maria Carter')
  })

  it('still cuts the quoted original', () => {
    const parts = splitHtmlEmail(html)
    expect(parts.body).not.toContain('the earlier message')
    expect(parts.quoted).toContain('the earlier message')
  })
})
