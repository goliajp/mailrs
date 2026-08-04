// The React Query cache surgery the mail mutations share: the optimistic
// patch, its rollback, the sticky-unread set, and the four invalidations.

import type { ListAxes } from '@/lib/list-membership'
import type { ConversationSummary } from '@/lib/types'

import { type QueryKey } from '@tanstack/react-query'
import { getDefaultStore } from 'jotai'

import { belongsTo } from '@/lib/list-membership'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'
import { stickyUnreadIdsAtom } from '@/store/ui'
// v2.1 §7 batch 1 (2026-07-08): every mutation path routes through
// the wire adapter — Zod-parsed responses, structured errors, 204
// handled explicitly.

export type InfinitePages = {
  pageParams: (number | undefined)[]
  pages: ConversationSummary[][]
}

export function addStickyUnread(threadId: string) {
  const store = getDefaultStore()
  const next = new Set(store.get(stickyUnreadIdsAtom))
  next.add(threadId)
  store.set(stickyUnreadIdsAtom, next)
}

// ---- delete (single + batch share the same backend) ----

// A bucket move (junk / not-junk / notification / promotion / inbox)
// changes WHICH list a thread belongs to, so the destination list — a
// different folder than the one on screen, therefore an INACTIVE query —
// must refetch too. The default `refetchType: 'active'` only refetches
// the mounted list, which is why the moved thread showed up in the
// target folder only after a hard refresh (2026-07-16). `'all'` refetches
// active + inactive conversation lists so switching to the target folder
// shows the thread immediately. Safe here: the backend has already moved
// the thread, so refetching the source list returns it correctly absent.
export function invalidateBucketMove() {
  queryClient
    .invalidateQueries({ queryKey: conversationKeys.all(), refetchType: 'all' })
    .catch(() => {})
  queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
}

// Invalidates ONLY list-shape queries — never the thread query.
//
// Read/unread/star/pin/archive/etc. don't change the message content of a
// thread; the thread's html_body / text_body / attachments / message
// metadata are identical pre- and post-mutation. Invalidating the thread
// query forced a refetch that returned byte-identical data, which then
// fed the HtmlFrame `srcDoc` through DOMPurify + proxyExternalUrls +
// injectCjkFonts + stripTrackingPixels a second time (50-300ms each
// iteration on newsletter bodies) and made every mark-as-read feel like
// the email was reloading. Thread cache invalidation lives in
// `use-mail-events.ts` (NewMessage WebSocket event) where the thread
// content actually does change.
export function invalidateMail() {
  queryClient.invalidateQueries({ queryKey: mailKeys.conversations() }).catch(() => {})
  queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
  // Deleting/archiving a thread that has sent mail in it also removes
  // rows from the per-message Send view — refetch so SendList doesn't
  // stale-display messages whose thread has already gone.
  queryClient.invalidateQueries({ queryKey: mailKeys.sent() }).catch(() => {})
  // v2.1 phase-3 — after the mail list migrated onto
  // `conversationKeys.infinite`, we broaden the invalidation to the
  // whole `conversation` entity namespace so both list + infinite
  // sub-caches refetch on the same trip. Cross-screen consistency
  // holds regardless of which screen a caller is on.
  queryClient.invalidateQueries({ queryKey: conversationKeys.all() }).catch(() => {})
}

// ---- batch operations ----

// Invalidates only the small server-computed aggregate (categories) —
// leaves the conversations list cache alone. Used by mark-read /
// mark-unread, where the optimistic patch already matches what the
// server returns; a list refetch races against the post-POST
// processing window and can flip the row back to unread for 100-500 ms.
export function invalidateMailAggregatesOnly() {
  queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
  // v2.1 phase-3 — cover the non-paginated `list` sub-namespace so
  // dashboard / sidebar aggregates recompute. The `infinite` cache is
  // left alone here (mark-read's optimistic patch already matches
  // server truth; a race-refetch would flicker rows back to unread).
  queryClient.invalidateQueries({ queryKey: conversationKeys.lists() }).catch(() => {})
}

/**
 * Apply a **field** patch to every cached list, then let each list drop
 * the rows that no longer belong in it.
 *
 * The two halves used to be one: each mutation returned `null` from its
 * patch to make a row disappear, which meant every mutation carried its
 * own opinion of which lists a moved thread leaves. `mark junk` dropped
 * it from all of them including Junk, so the row left the screen and the
 * refetch put it back; `archive` dropped it from none and relied on a
 * client-side filter that the server made redundant on 2026-08-05, so
 * archiving from the Inbox left the row sitting there.
 *
 * Now a mutation writes what its endpoint writes and `belongsTo` decides
 * the rest, per cache line, from that line's own axes.
 */
export function patchConversationFields(
  patch: (c: ConversationSummary) => ConversationSummary
): Array<[QueryKey, InfinitePages | undefined]> {
  const snapshots: Array<[QueryKey, InfinitePages | undefined]> = []
  for (const prefix of [mailKeys.conversations(), conversationKeys.infinites()]) {
    for (const [key, data] of queryClient.getQueriesData<InfinitePages>({ queryKey: prefix })) {
      snapshots.push([key, data])
      if (!data) continue
      const axes = axesOfCacheKey(key)
      queryClient.setQueryData<InfinitePages>(key, {
        ...data,
        pages: data.pages.map((page) =>
          page.map(patch).filter((c) => axes === null || belongsTo(axes, c))
        ),
      })
    }
  }
  return snapshots
}

export function patchConversations(
  patch: (c: ConversationSummary) => ConversationSummary | null
): Array<[QueryKey, InfinitePages | undefined]> {
  // v2.1 phase-3: patch every cache line under both the legacy
  // `mailKeys.conversations()` prefix AND the new
  // `conversationKeys.infinites()` prefix. `useConversationsQuery`
  // (the mail-list) moved onto the new key; the old key survives only
  // for callers not yet migrated. Both are snapshotted so rollback
  // returns each cache line to its exact pre-mutation state.
  const applyPatch = (old: InfinitePages | undefined): InfinitePages | undefined => {
    if (!old) return old
    return {
      ...old,
      pages: old.pages.map((page) => {
        const next: ConversationSummary[] = []
        for (const c of page) {
          const updated = patch(c)
          if (updated !== null) next.push(updated)
        }
        return next
      }),
    }
  }
  const snapshots: Array<[QueryKey, InfinitePages | undefined]> = []
  for (const prefix of [mailKeys.conversations(), conversationKeys.infinites()]) {
    const entries = queryClient.getQueriesData<InfinitePages>({ queryKey: prefix })
    for (const entry of entries) snapshots.push(entry)
    queryClient.setQueriesData<InfinitePages>({ queryKey: prefix }, applyPatch)
  }
  return snapshots
}

export function removeStickyUnread(threadId: string) {
  const store = getDefaultStore()
  const current = store.get(stickyUnreadIdsAtom)
  if (!current.has(threadId)) return
  const next = new Set(current)
  next.delete(threadId)
  store.set(stickyUnreadIdsAtom, next)
}

export function rollbackConversations(snapshots: Array<[QueryKey, InfinitePages | undefined]>) {
  for (const [key, data] of snapshots) queryClient.setQueryData(key, data)
}

/**
 * The axes a cache line was fetched with, read back off its own key.
 *
 * Both key shapes end in the filter object — `conversationKeys.infinite`
 * with `canonicaliseFilter`'s output, `mailKeys.conversations` with
 * `normalizeFilters`' — and both spell these four the same way. A key
 * whose tail is neither returns null, and such a line is patched without
 * being pruned rather than being emptied on a guess.
 */
function axesOfCacheKey(key: QueryKey): ListAxes | null {
  const tail = key[key.length - 1]
  if (typeof tail !== 'object' || tail === null) return null
  const f = tail as Record<string, unknown>
  return {
    archived: f.archived === true || f.archived === 1,
    folder: typeof f.folder === 'string' ? f.folder : null,
    starred: f.starred === true || f.starred === 1 ? true : null,
    unread: f.unread === true || f.unread === 1 ? true : null,
  }
}
