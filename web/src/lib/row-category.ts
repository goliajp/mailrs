import type { ConversationSummary } from '@/lib/types'

type Placed = Pick<
  ConversationSummary,
  | 'archived'
  | 'category'
  | 'message_count'
  | 'quarantined'
  | 'sent_count'
  | 'snoozed_until'
  | 'trashed_at'
>

/**
 * Whether the conversation is in the reader's Inbox list now, by the rules
 * the server lists it by: not archived, held, in the Trash or snoozed, and
 * not one the reader only ever sent into.
 */
export function inInbox(c: Placed, nowSecs: number): boolean {
  const sentOnly = c.message_count > 0 && c.sent_count >= c.message_count
  if (sentOnly || c.archived || c.quarantined) return false
  if ((c.trashed_at ?? 0) > 0) return false
  return (c.snoozed_until ?? 0) <= nowSecs
}

/**
 * The category a row shows, or null for none. "inbox" names a place, not
 * a kind of mail, so it is shown only on a conversation that is in the
 * Inbox; the other categories say what the mail is, wherever it lies.
 */
export function rowCategory(c: Placed, nowSecs: number): null | string {
  if (!c.category || c.category === 'general') return null
  if (c.category === 'inbox' && !inInbox(c, nowSecs)) return null
  return c.category
}
