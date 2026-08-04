import type { ConversationSummary } from '@/lib/types'
import type { ImportanceSection, QuickFilter, SortOrder } from '@/store/ui'

/**
 * The client-side narrowing the conversation list applies on top of the
 * page the server returned — the rows it actually draws.
 *
 * Pure and out here because "the first row of this list" has to mean the
 * same thing to the list and to the reading pane, and it lived inside a
 * `useMemo` in `ConversationList` where only the list could see it.
 *
 * The archive filter that used to head this function is gone: the server
 * excludes archived threads from every list but Archived since
 * 2026-08-05, so pruning the page here only ever made its size disagree
 * with the count it came with.
 */
export function narrowConversations(
  conversations: readonly ConversationSummary[],
  opts: {
    importanceSection: ImportanceSection
    quickFilter: QuickFilter
    sortOrder: SortOrder
    stickyUnread: ReadonlySet<string>
  }
): ConversationSummary[] {
  let visible = [...conversations]

  if (opts.quickFilter === 'unread') {
    // Gmail-style: a thread marked read while the user is sitting on this
    // list stays visible until they leave it — a row should never vanish
    // under the cursor. `stickyUnread` is cleared when the list changes.
    visible = visible.filter((c) => c.unread_count > 0 || opts.stickyUnread.has(c.thread_id))
  } else if (opts.quickFilter === 'starred') {
    visible = visible.filter((c) => c.flagged)
  }

  if (opts.importanceSection === 'important') {
    visible = visible.filter(
      (c) => c.importance_level === 'critical' || c.importance_level === 'important'
    )
  } else if (opts.importanceSection === 'other') {
    visible = visible.filter((c) => c.importance_level === 'low' || c.importance_level === 'noise')
  }

  // `relevance` means "leave the server's order alone". For a plain list
  // that order is already newest-first, so the two agree; for a search it
  // is the ranking, and sorting it by date would discard the ranking.
  if (opts.sortOrder === 'relevance') return visible

  const pinned = visible.filter((c) => c.pinned)
  const unpinned = visible.filter((c) => !c.pinned)
  if (opts.sortOrder === 'newest') {
    unpinned.sort((a, b) => b.last_date - a.last_date)
  } else if (opts.sortOrder === 'oldest') {
    unpinned.sort((a, b) => a.last_date - b.last_date)
  } else if (opts.sortOrder === 'unread') {
    unpinned.sort((a, b) => b.unread_count - a.unread_count || b.last_date - a.last_date)
  }
  return [...pinned, ...unpinned]
}
