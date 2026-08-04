/**
 * `useFlatConversations` — layer-4 façade that projects
 * `conversationKeys.infinite(filter)` down to a flat, dedup'd array
 * with identity-preserving merge.
 *
 * v2.1 phase-5 reader migration:
 *   - Callers previously did `useAtomValue(conversationsAtom)` and
 *     depended on `chat.tsx`'s effect keeping the atom in sync with the
 *     RQ cache.
 *   - The atom was a shadow copy of the same data — a source of the
 *     class of "one screen updated, another didn't" bugs the v2.1
 *     redesign eliminates (RFC §1).
 *   - This hook reads the RQ cache DIRECTLY. When a mutation patches
 *     `conversationKeys.infinites()`, every caller re-renders on the
 *     next tick with the updated flat array — no atom, no sync
 *     effect, no drift.
 *
 * Identity preservation across renders:
 *   The hook uses a ref to remember the last returned array; when the
 *   flatten produces an entry that is `shallowEqualConvo`-equivalent
 *   to the previously-returned one, the ref's entry is kept. This
 *   preserves object identity for `memo`d rows, so a background
 *   refetch that returns byte-identical data doesn't re-render every
 *   `<ConversationRow />`.
 */

import type { ConversationSummary } from '@/lib/types'

import { useCallback, useMemo, useRef } from 'react'

import { shallowEqualConvo } from '@/hooks/use-mail-events'
import { useConversationsQuery } from '@/hooks/use-mail-queries'
import { type MailListFilters } from '@/lib/query-keys'

/**
 * `filters` is nullable because the list on screen may not be served by
 * `/conversations` at all — Send and Draft have their own endpoints. A
 * null filter disables the query rather than substituting an Inbox one,
 * which would fetch a list nobody is looking at and hand its first row
 * to the reading pane.
 */
export function useFlatConversations(
  filters: MailListFilters | null,
  enabled: boolean = true
): {
  conversations: ConversationSummary[]
  hasMore: boolean
  initialLoading: boolean
  loadingMore: boolean
  loadMore: () => Promise<void>
  refresh: () => Promise<void>
} {
  const query = useConversationsQuery(filters ?? {}, enabled && filters !== null)
  const prevFlat = useRef<ConversationSummary[]>([])

  const conversations = useMemo(() => {
    const pages = query.data?.pages ?? []
    const seen = new Set<string>()
    const flat: ConversationSummary[] = []
    for (const page of pages) {
      for (const c of page) {
        if (seen.has(c.thread_id)) continue
        seen.add(c.thread_id)
        flat.push(c)
      }
    }
    // Identity-preserving merge: for each row, if the previously
    // rendered row is shallowly equal, keep the old reference so
    // memo'd children don't re-render.
    const prev = prevFlat.current
    const byId = new Map<string, ConversationSummary>()
    for (const c of prev) byId.set(c.thread_id, c)
    let allSame = prev.length === flat.length
    const merged = flat.map((f, i) => {
      const existing = byId.get(f.thread_id)
      const kept = existing && shallowEqualConvo(existing, f) ? existing : f
      if (allSame && prev[i] !== kept) allSame = false
      return kept
    })
    const out = allSame ? prev : merged
    prevFlat.current = out
    return out
  }, [query.data])

  const initialLoading = query.isPending && conversations.length === 0
  const hasMore = query.hasNextPage ?? false
  const loadingMore = query.isFetchingNextPage

  // The paging and refresh handlers travel with the rows. They used to
  // be built in `chat.tsx` off a second copy of this query and passed
  // down as props, which is how the page the list drew and the page
  // `loadMore` extended could be two different queries.
  const loadMore = useCallback(async () => {
    if (!query.hasNextPage || query.isFetchingNextPage) return
    await query.fetchNextPage()
  }, [query])
  const refresh = useCallback(async () => {
    await query.refetch()
  }, [query])

  return { conversations, hasMore, initialLoading, loadingMore, loadMore, refresh }
}
