// Who is coming, in the words a reader uses. Rules rather than
// components — split out of invite-card when it crossed this repo's
// 500-line limit, and the lint that forbids a component file exporting
// plain functions was already saying the same thing.

import type { Attendee } from '@/lib/invite-types'

import { tNow } from '@/i18n'

/// The one-word verdict on a timeline row, where there is space for a
/// pill and not for a sentence.
export function compactStatusLabel(partstat: string): null | { className: string; label: string } {
  switch (partstat) {
    case 'ACCEPTED':
      return {
        className: 'bg-emerald-500/15 text-emerald-300',
        label: tNow('invite.guest.accepted'),
      }
    case 'DECLINED':
      return { className: 'bg-red-500/15 text-red-300', label: tNow('invite.guest.declined') }
    case 'TENTATIVE':
      return { className: 'bg-amber-500/15 text-amber-300', label: tNow('invite.guest.tentative') }
    default:
      return null
  }
}

export function guestSummary(attendees: Attendee[]): string {
  const n = attendees.length
  const head = guestCount(n)
  const yes = attendees.filter((a) => a.partstat.toUpperCase() === 'ACCEPTED').length
  const no = attendees.filter((a) => a.partstat.toUpperCase() === 'DECLINED').length
  const waiting = attendees.filter((a) => a.partstat.toUpperCase() === 'NEEDS-ACTION').length
  const parts: string[] = []
  if (yes) parts.push(tNow('invite.guest.yesCount', { n: yes }))
  if (no) parts.push(tNow('invite.guest.noCount', { n: no }))
  if (waiting) parts.push(tNow('invite.guest.awaitingCount', { n: waiting }))
  if (parts.length === 0) return head
  return `${head} · ${parts.join(tNow('invite.guest.separator'))}`
}

export function partstatDot(partstat: string): string {
  switch (partstat.toUpperCase()) {
    case 'ACCEPTED':
      return 'text-emerald-400'
    case 'DECLINED':
      return 'text-rose-400'
    case 'TENTATIVE':
      return 'text-amber-400'
    default:
      return 'text-fg-muted/40'
  }
}

export function partstatWord(partstat: string): string {
  switch (partstat.toUpperCase()) {
    case 'ACCEPTED':
      return tNow('invite.guest.yes')
    case 'DECLINED':
      return tNow('invite.guest.no')
    case 'TENTATIVE':
      return tNow('invite.guest.maybe')
    default:
      return tNow('invite.guest.awaiting')
  }
}

function guestCount(n: number): string {
  if (n === 1) return tNow('invite.guest.one')
  return tNow('invite.guest.many', { n })
}
