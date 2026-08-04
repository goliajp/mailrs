import type { ConversationSummary } from '@/lib/types'

/**
 * Whether a row belongs in a list, from the row's own fields.
 *
 * This exists so an optimistic update can write **what the API it calls
 * actually changes** — `archive` writes `archived: true`, `mark junk`
 * writes `category: 'spam'` — and let membership follow, instead of each
 * mutation deciding by hand which cache lines the row should vanish
 * from. Deciding by hand is why `mark junk` dropped the row from every
 * list including Junk itself: the row left the screen and the refetch
 * put it back.
 *
 * Only the axes a mutation can move are answered here. `unread`
 * deliberately is not: a thread marked read while the Unread list is
 * open stays visible until you leave it (Gmail's behaviour, and the
 * sticky-unread set in `narrowConversations` is what implements it), so
 * evicting it from the cache would fight that.
 */

/** The bucket a category files under. */
export type Bucket = 'inbox' | 'junk' | 'notifications' | 'promotions'

/** The axes of a cached list, as its query key records them. */
export type ListAxes = {
  archived: boolean
  folder: null | string
  starred: boolean | null
  unread: boolean | null
}

export function belongsTo(axes: ListAxes, row: ConversationSummary): boolean {
  // Archived is exclusive in both directions: it is a list of its own,
  // so an archived thread is in it and in nothing else.
  if (row.archived !== axes.archived) return false
  if (axes.starred === true && !row.flagged) return false

  const bucket = bucketOf(row.category)
  // Case-insensitively, as the server reads it
  // (`ListThreadsFilter::scope`, which is `eq_ignore_ascii_case` on
  // every arm). The UI spells these capitalised and the wire does not
  // care, so neither should this.
  switch (axes.folder?.toLowerCase()) {
    case 'inbox':
      return bucket === 'inbox'
    case 'junk':
      return bucket === 'junk'
    // Not a folder anyone navigates to — the scope the Unread and
    // Starred lists ask for, which is everything but Junk.
    case 'nonjunk':
      return bucket !== 'junk'
    case 'np':
      return bucket === 'notifications' || bucket === 'promotions'
    // Drafts, Sent and Trash are not served by this cache, and a null
    // folder is the unscoped list. Neither has anything to decide.
    default:
      return true
  }
}

/**
 * Mirror of `keys::bucket_of` —
 * `crates/mailbox-kevy/src/keys/threads.rs:228`. A second copy of a
 * server rule, which is a cost; the alternative was fifteen mutations
 * each guessing which lists a moved thread leaves, which is fifteen
 * copies of a rule nobody wrote down. `bucket_of_matches_the_server`
 * pins the pairs.
 */
export function bucketOf(category: string): Bucket {
  const c = category.toLowerCase()
  if (c === 'spam' || c === 'scam') return 'junk'
  if (c === 'notification' || c === 'notifications') return 'notifications'
  if (c === 'promotion' || c === 'promotions') return 'promotions'
  return 'inbox'
}
