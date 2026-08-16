import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SenderTrustBadge } from '../sender-trust-badge'

/**
 * **A check mark is an identity claim, and DMARC is not identity.**
 *
 * Reported 2026-08-16 with a screenshot: a green "Verified sender"
 * shield beside `MyJCB` on a JCB phishing mail, in Junk. Measured, the
 * message really did pass everything — `spf=pass dkim=pass dmarc=pass`
 * — because `wokjx.crabfishhh.com` is the attacker's own domain and
 * publishing authentication records for a domain you control is free.
 *
 * DMARC's claim is "this mail came from the domain in the From header",
 * and it was true. The lie was the display name, which DMARC does not
 * authenticate. So the badge answered a question nobody asked, beside
 * the words that were the deception.
 *
 * Gmail's check mark requires DMARC at an enforced policy, BIMI, **and
 * a Verified Mark Certificate** — a CA verifying trademark ownership.
 * The mark stands for an identity checked out of band, never for a
 * passing authentication run. We have no VMC, so we show no mark.
 *
 * The asymmetry is the whole design: a warning that is sometimes wrong
 * costs a user distrusting real mail; a check that is sometimes wrong
 * costs a user trusting a fake one, and the attacker chose it.
 */
describe('SenderTrustBadge', () => {
  it('shows nothing for a DMARC pass', () => {
    const { container } = render(<SenderTrustBadge trust="verified" />)
    expect(container).toBeEmptyDOMElement()
  })

  it('still warns on a failed check', () => {
    render(<SenderTrustBadge trust="suspicious" />)
    expect(screen.getByText(/unverified sender/i)).toBeInTheDocument()
  })

  it('shows nothing for the unremarkable middle', () => {
    for (const t of ['unverified', '', 'anything-else']) {
      const { container } = render(<SenderTrustBadge trust={t} />)
      expect(container).toBeEmptyDOMElement()
    }
  })
})
