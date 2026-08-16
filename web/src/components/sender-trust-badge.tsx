import { ShieldAlert } from 'lucide-react'

/**
 * Sender-authentication badge, from the message's `sender_trust` field
 * (self-hosted SPF/DKIM/DMARC verdict — see
 * `mailrs_inbound::sender_trust`).
 *
 * **Warnings only. There is no positive mark, and that is deliberate.**
 *
 * There was one: `verified` drew a green shield on a DMARC pass. A JCB
 * phishing mail earned it on 2026-08-16 — `spf=pass dkim=pass
 * dmarc=pass`, all correct, because `wokjx.crabfishhh.com` is the
 * attacker's own domain and authentication records are free on a domain
 * you control. DMARC's claim is that the mail came from the domain in
 * the From header; the lie was the display name, which DMARC does not
 * authenticate. The badge answered a question nobody asked, beside the
 * words that were the deception.
 *
 * Gmail's check mark requires DMARC at an enforced policy, BIMI, and a
 * **Verified Mark Certificate** — a CA verifying trademark ownership.
 * It stands for an identity checked out of band, never for a passing
 * authentication run. We have no VMC, so we show no mark.
 *
 * The asymmetry is the design: a warning that is sometimes wrong costs
 * a user distrusting real mail; a mark that is sometimes wrong costs a
 * user trusting a fake one, and the attacker chose it.
 */
export function SenderTrustBadge({ trust }: { trust: string }) {
  switch (trust) {
    case 'suspicious':
      return (
        <span
          className="bg-danger/10 text-danger inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] font-medium"
          title="This sender does not hold up: either its authentication failed, or its displayed name was tampered with"
        >
          <ShieldAlert className="h-3 w-3" />
          Suspicious sender
        </span>
      )
    default:
      return null
  }
}
