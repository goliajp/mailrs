import type { UnreadCounts } from '@/hooks/use-current-mail-filters'
import type { MailListId } from '@/lib/mail-lists'

/**
 * The unread conversations a folder lists. The Inbox used to show the
 * whole count, which takes in Notifications & Promotions as well: two
 * unread promotions read as two unread in the Inbox.
 */
export function countFor(id: MailListId, unread: UnreadCounts): number {
  switch (id) {
    case 'inbox':
      return unread.inbox
    case 'np':
      return unread.np
    case 'unread':
      return unread.count
    default:
      return 0
  }
}
