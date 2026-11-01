import { extractName } from '@/lib/avatar'
import { splitAddressList } from '@/lib/recipients'

// format recipients string into a short human-readable form
export function formatRecipients(recipients: string): string {
  const parts = splitAddressList(recipients).map(extractName).filter(Boolean)
  if (parts.length === 0) return recipients
  if (parts.length <= 2) return parts.join(', ')
  return `${parts[0]}, ${parts[1]} +${parts.length - 2}`
}
