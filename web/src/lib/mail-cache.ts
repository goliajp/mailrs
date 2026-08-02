// The React Query cache surgery the mail mutations share: the optimistic
// patch, its rollback, the sticky-unread set, and the four invalidations.

import type { ConversationSummary } from '@/lib/types'

import { type QueryKey } from '@tanstack/react-query'
import { getDefaultStore } from 'jotai'

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
