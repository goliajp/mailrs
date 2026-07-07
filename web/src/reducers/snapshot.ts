/**
 * Snapshot / rollback helpers — layer 3 (see RFC §2.3).
 *
 * Command handlers call `snapshotMatching` before every optimistic
 * patch. On error they call `restoreSnapshot`. On success they let
 * the natural refetch reconcile.
 *
 * These are the only rollback primitives; command handlers do NOT
 * manage rollback state by hand.
 */

import type { ConversationSummary } from '@/lib/types'
import type { InfiniteData, QueryClient, QueryKey } from '@tanstack/react-query'

import { conversationKeys } from '@/store/query-keys-v21'

export type Snapshot = ReadonlyArray<readonly [QueryKey, unknown]>

/**
 * Phase-5c helper — walk every `conversationKeys.infinites()` cache
 * line (one per filter combination the mail list has queried) and
 * apply `updater` to each ConversationSummary. Return `null` from
 * the updater to drop the row (used by delete).
 *
 * Replaces the historical `setConversations((prev) => prev.map(...))`
 * pattern that lived across `use-keyboard-nav.ts` and
 * `conversation-list.tsx`. Every optimistic mutation on the mail
 * list now flows through this helper so every reader — dashboard
 * badge, filter chip counts, `useFlatConversations`-backed screens —
 * sees the change in one paint.
 */
export function patchAllInfiniteLists(
  qc: QueryClient,
  updater: (c: ConversationSummary) => ConversationSummary | null
): void {
  const entries = qc.getQueriesData<InfiniteData<ConversationSummary[]>>({
    queryKey: conversationKeys.infinites(),
  })
  for (const [key, data] of entries) {
    if (!data) continue
    qc.setQueryData<InfiniteData<ConversationSummary[]>>(key, {
      ...data,
      pages: data.pages.map((page) => {
        const next: ConversationSummary[] = []
        for (const c of page) {
          const updated = updater(c)
          if (updated !== null) next.push(updated)
        }
        return next
      }),
    })
  }
}

/**
 * Patch every cache entry matching `keyPrefix` with the same pure
 * updater. Guarantees that when a mutation applies to N cache lines
 * (e.g. the same thread appears in "Inbox" and "Starred" lists) all N
 * get patched consistently.
 */
export function patchMatching<T>(
  qc: QueryClient,
  keyPrefix: QueryKey,
  updater: (data: T) => T
): void {
  const entries = qc.getQueriesData<T>({ queryKey: keyPrefix })
  for (const [key, data] of entries) {
    if (data === undefined) continue
    qc.setQueryData<T>(key, updater(data))
  }
}

/**
 * Reverse of `snapshotMatching`. Restores every captured entry with
 * its exact pre-mutation value.
 */
export function restoreSnapshot(qc: QueryClient, snapshot: Snapshot): void {
  for (const [key, data] of snapshot) {
    qc.setQueryData(key, data)
  }
}

/**
 * Capture every current cache entry matching `keyPrefix` before an
 * optimistic mutation. Returned Snapshot is opaque to callers.
 */
export function snapshotMatching(qc: QueryClient, keyPrefix: QueryKey): Snapshot {
  const entries = qc.getQueriesData({ queryKey: keyPrefix })
  return entries.map(([key, data]) => [key, data] as const)
}
