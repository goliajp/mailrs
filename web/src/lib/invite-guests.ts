// Who is coming, in the words a reader uses. Rules rather than
// components — split out of invite-card when it crossed this repo's
// 500-line limit, and the lint that forbids a component file exporting
// plain functions was already saying the same thing.

import type { Attendee } from '@/lib/invite-types'

export function guestSummary(attendees: Attendee[]): string {
  const n = attendees.length
  const head = `${n} guest${n === 1 ? '' : 's'}`
  const yes = attendees.filter((a) => a.partstat.toUpperCase() === 'ACCEPTED').length
  const no = attendees.filter((a) => a.partstat.toUpperCase() === 'DECLINED').length
  const waiting = attendees.filter((a) => a.partstat.toUpperCase() === 'NEEDS-ACTION').length
  const parts: string[] = []
  if (yes) parts.push(`${yes} yes`)
  if (no) parts.push(`${no} no`)
  if (waiting) parts.push(`${waiting} awaiting`)
  return parts.length > 0 ? `${head} · ${parts.join(', ')}` : head
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
      return 'yes'
    case 'DECLINED':
      return 'no'
    case 'TENTATIVE':
      return 'maybe'
    default:
      return 'awaiting'
  }
}
