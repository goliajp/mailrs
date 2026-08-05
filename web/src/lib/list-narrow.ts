import type { SortOrder } from '@/store/ui'

/** Epoch seconds from the several shapes a draft timestamp arrives in. */
export function draftDate(d: {
  created_at: null | number | string
  updated_at: null | number | string
}): number {
  const raw = d.updated_at ?? d.created_at
  if (raw === null) return 0
  if (typeof raw === 'number') return raw
  const parsed = Date.parse(raw)
  return Number.isNaN(parsed) ? 0 : Math.floor(parsed / 1000)
}

/**
 * The two refinements every list applies, for the sources whose rows are
 * not conversations.
 *
 * `narrowConversations` does this for the thread lists and has always
 * done it. Send and Draft each grew their own — a private query atom
 * matched with `includes`, and no sort at all — so the same search box
 * meant something different depending on which chip was lit, and the
 * sort control rendered above them did nothing. The registry says the
 * screen treats all eight lists the same way; these are the two axes
 * that never got there.
 *
 * How a source *answers* the query is still its own business: the thread
 * lists send it to the server's staged search, these two match locally.
 * What is shared is that there is one question.
 */
export function matchesQuery(query: string, fields: readonly string[]): boolean {
  const q = query.trim().toLowerCase()
  if (!q) return true
  return fields.some((f) => f.toLowerCase().includes(q))
}

/**
 * Sort rows that have a date and nothing else to sort on.
 *
 * Two of the four orders have no meaning here and both fall back to
 * newest: `relevance` is a server ranking these rows never get (they are
 * matched locally, so every match is equally good), and neither a send
 * nor a draft can be `unread`. Falling back is the honest answer —
 * leaving the order alone would hand back the source's join order under
 * a name that promises a ranking.
 */
export function sortByDate<T>(rows: readonly T[], date: (row: T) => number, order: SortOrder): T[] {
  const out = [...rows]
  out.sort((a, b) => (order === 'oldest' ? date(a) - date(b) : date(b) - date(a)))
  return out
}
