import type { WireReason } from '@/wire/schemas/reason'

import { useAtomValue } from 'jotai'
import { ShieldAlert } from 'lucide-react'

import { langAtom, useT } from '@/i18n'
import { reasonText } from '@/lib/reasons'

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
  const t = useT()
  switch (trust) {
    case 'suspicious':
      return (
        <span
          className="bg-danger/10 text-danger inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] font-medium"
          title={t('trust.suspiciousNote')}
        >
          <ShieldAlert className="h-3 w-3" />
          {t('trust.suspicious')}
        </span>
      )
    default:
      return null
  }
}

const EMPTY_CHECKS: readonly WireReason[] = []

/**
 * Why the sender is suspicious: what SPF, DKIM and DMARC each said, and
 * whether the name renders reordered. Shown beside the warning badge, so
 * the reader sees which check failed rather than only that one did.
 */
export function SenderChecks({ checks = EMPTY_CHECKS }: { checks?: readonly WireReason[] }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  if (checks.length === 0) return null
  return (
    <p className="flex min-h-4 flex-wrap items-center gap-x-2" data-testid="sender-checks">
      {checks.map((r) => (
        <span className={checkClass(r)} key={`${r.code}:${r.params?.check ?? ''}`}>
          {reasonText(r, lang, t) ?? r.code}
        </span>
      ))}
    </p>
  )
}

function checkClass(r: WireReason): string {
  if (r.code === 'auth-check' && r.params?.result === 'pass') return 'text-fg-muted'
  return 'text-danger'
}
