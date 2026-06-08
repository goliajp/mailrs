import { extractName } from '@/lib/avatar'
import { dateGroupLabel } from '@/lib/format'

export const bubbleDateLabel = (ts: number | string) =>
  dateGroupLabel(typeof ts === 'number' ? ts : Math.floor(new Date(ts).getTime() / 1000))

// format recipients string into a short human-readable form
export function formatRecipients(recipients: string): string {
  const parts = recipients
    .split(',')
    .map((r) => extractName(r.trim()))
    .filter(Boolean)
  if (parts.length === 0) return recipients
  if (parts.length <= 2) return parts.join(', ')
  return `${parts[0]}, ${parts[1]} +${parts.length - 2}`
}
