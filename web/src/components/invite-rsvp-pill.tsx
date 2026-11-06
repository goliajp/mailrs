import { type Key, useT } from '@/i18n'
import { formatEpochOrIso } from '@/lib/invite-time'

/// The answer this reader already gave, and when.
export function RsvpStatusPill({ at, partstat }: { at: null | string; partstat: string }) {
  const t = useT()
  const label = t(replyLabel(partstat), { status: partstat })
  const when = formatEpochOrIso(at)
  return (
    <span
      className={`flex items-center gap-1 rounded px-2.5 py-1 text-xs font-medium ${pillClass(partstat)}`}
    >
      {label}
      {when && <span className="text-fg-muted ml-1 font-normal">{when}</span>}
    </span>
  )
}

function pillClass(partstat: string): string {
  if (partstat === 'ACCEPTED') return 'bg-emerald-500/15 text-emerald-300'
  if (partstat === 'DECLINED') return 'bg-red-500/15 text-red-300'
  return 'bg-amber-500/15 text-amber-300'
}

function replyLabel(partstat: string): Key {
  if (partstat === 'ACCEPTED') return 'invite.replied.accepted'
  if (partstat === 'TENTATIVE') return 'invite.replied.tentative'
  if (partstat === 'DECLINED') return 'invite.replied.declined'
  return 'invite.replied.other'
}
