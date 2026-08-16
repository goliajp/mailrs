import { HelpCircle } from 'lucide-react'

import { contradictedDomain } from '@/lib/sender-claim'

/**
 * Where a message actually came from, when its sender's name says
 * somewhere else.
 *
 * The reading pane shows a display name and rarely an address, which is
 * the gap brand impersonation lives in. Measured on this deployment's
 * 33,583 stored messages, this fires on **83** of them (0.247%) — see
 * `@/lib/sender-claim` for the measurement and for why it states rather
 * than accuses.
 *
 * It is the companion to `SenderTrustBadge`, not a replacement: that one
 * reports what the server's checks concluded, this one reports a
 * disagreement the checks have no opinion about. A phish on a domain the
 * attacker owns passes every check and still trips this.
 */
export function SenderClaimBadge({ sender }: { sender: string }) {
  const actual = contradictedDomain(sender, sender)
  if (!actual) return null
  return (
    <span
      className="bg-warning/10 text-warning inline-flex max-w-[12rem] items-center gap-0.5 rounded px-1 py-0.5 text-[10px] font-medium"
      title={`This message was sent from ${actual}, which is not the domain its sender's name claims`}
    >
      <HelpCircle className="h-3 w-3 shrink-0" />
      <span className="truncate">{actual}</span>
    </span>
  )
}
