import { describe, expect, it } from 'vitest'

import { splitEmail } from '../email-split'

/**
 * Whatever the splitter cuts out has to remain reachable.
 *
 * `MessageBubble` rendered `parts.body` and discarded `parts.quoted`
 * with no way to ask for it, so an Outlook forward arrived showing the
 * covering note — "Hi Li, can you take a look soon?" — and none of the
 * chain it was forwarding. Reported 2026-08-12 against a message that
 * read in full on iOS, which folds the same content behind a toggle.
 *
 * These assert the split itself: the covering note and the forwarded
 * chain both survive, in the right halves. The component test for the
 * toggle lives beside the component.
 */
describe('an Outlook forward keeps its chain', () => {
  const html = `
    <div>Hi Li,<br><br>Can you take a look soon?<br><br>Best,<br>Chris</div>
    <div id="divRplyFwdMsg">
      <b>From:</b> Robin &lt;rrobin@qti.qualcomm.com&gt;<br>
      <b>Subject:</b> Re: Query on device status caching
    </div>
    <div>Thanks Minhao. We will take a look.</div>
  `

  it('the covering note is the body', () => {
    const { parts } = splitEmail(null, html)
    expect(parts.body).toContain('Can you take a look soon?')
  })

  it('the forwarded chain is kept, not thrown away', () => {
    const { parts } = splitEmail(null, html)
    expect(parts.quoted, 'the chain was dropped entirely').not.toBeNull()
    expect(parts.quoted).toContain('rrobin@qti.qualcomm.com')
    expect(parts.quoted, 'the deepest part of the chain went missing').toContain(
      'We will take a look'
    )
  })

  /**
   * Nothing may vanish between the two halves: what the reader can get
   * to, expanded, is what arrived.
   */
  it('body plus quoted accounts for the message', () => {
    const { parts } = splitEmail(null, html)
    const seen = `${parts.body}${parts.quoted ?? ''}`
    for (const fragment of ['Can you take a look soon?', 'Robin', 'We will take a look']) {
      expect(seen, `${fragment} is in neither half`).toContain(fragment)
    }
  })
})
