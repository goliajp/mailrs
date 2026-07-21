import { ShieldAlert, ShieldCheck } from 'lucide-react'

/**
 * Sender-authentication badge, from the message's `sender_trust` field
 * (self-hosted SPF/DKIM/DMARC verdict — see
 * `mailrs_inbound::sender_trust`). Deliberately quiet: the value users
 * need to notice is `suspicious`, so that one is loud and labelled,
 * `verified` is a small unobtrusive check, and everything else renders
 * nothing rather than badging the vast, unremarkable middle.
 */
export function SenderTrustBadge({ trust }: { trust: string }) {
  switch (trust) {
    case 'suspicious':
      return (
        <span
          className="bg-danger/10 text-danger inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] font-medium"
          title="Sender authentication failed — the From address may be spoofed"
        >
          <ShieldAlert className="h-3 w-3" />
          Unverified sender
        </span>
      )
    case 'verified':
      return (
        <ShieldCheck
          aria-label="Verified sender (DMARC pass)"
          className="text-success h-3.5 w-3.5 shrink-0"
        />
      )
    default:
      return null
  }
}
