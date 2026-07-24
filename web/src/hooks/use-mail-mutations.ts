import type { ConversationSummary } from '@/lib/types'
import type { WireSentMessage } from '@/wire/schemas/mail'

import { type QueryKey, useMutation } from '@tanstack/react-query'
import { getDefaultStore } from 'jotai'

import { snoozeConversation as snoozeApi, unsnoozeConversation as unsnoozeApi } from '@/lib/api'
import { dedupeSentByMessageId } from '@/lib/dedupe-sent'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'
import { stickyUnreadIdsAtom } from '@/store/ui'
// v2.1 §7 batch 1 (2026-07-08): every mutation path routes through
// the wire adapter — Zod-parsed responses, structured errors, 204
// handled explicitly.
import {
  wireArchiveThread,
  wireBatchMutation,
  wireDeleteThread,
  wireMarkJunk,
  wireMarkNotification,
  wireMarkNotJunk,
  wireMarkPromotion,
  wireMarkThreadRead,
  wireMarkThreadUnread,
  wireMoveToInbox,
  wirePinThread,
  wireStarThread,
  wireUnarchiveThread,
  wireUnpinThread,
  wireUnstarThread,
} from '@/wire/endpoints/mutations'

export type BatchAction = 'archive' | 'delete' | 'read' | 'star' | 'unarchive' | 'unread' | 'unstar'

type BatchResult = {
  failed: number
  message?: string
  processed: number
  success: boolean
}

// Mutation hooks for the mail flow. Every one of them runs the same
// optimistic-update + rollback dance:
//
//   1. onMutate: cancel in-flight refetches so the optimistic write
//      isn't immediately stomped, snapshot every conversations query's
//      data, then patch each cached page through `patch` so the UI
//      updates instantly.
//   2. onError: restore every snapshot back into the cache.
//   3. onSettled: invalidate the conversations queries so the next
//      refetch reconciles against the server.
//
// Patching the cache directly (instead of writing to the legacy
// `conversationsAtom`) means the optimistic state survives any concurrent
// refetch — RQ's getQueryData / setQueryData operates on the canonical
// store, not on a React-state mirror.

type Context = { snapshots: Array<[QueryKey, InfinitePages | undefined]> }

type InfinitePages = { pageParams: (number | undefined)[]; pages: ConversationSummary[][] }

export function useArchiveMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireArchiveThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, archived: true } : c
      )
      return { snapshots }
    },
    onSettled: () => invalidateBucketMove(),
  })
}

export function useBatchMutation() {
  return useMutation<BatchResult, Error, { action: BatchAction; threadIds: string[] }, Context>({
    mutationFn: ({ action, threadIds }) => wireBatchMutation(action, threadIds),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ action, threadIds }) => {
      await cancelConversationFetches()
      const set = new Set(threadIds)
      const snapshots = patchConversations((c) => {
        if (!set.has(c.thread_id)) return c
        switch (action) {
          case 'archive':
            return { ...c, archived: true }
          case 'delete':
            return null
          case 'read':
            return { ...c, unread_count: 0 }
          case 'star':
            return { ...c, flagged: true }
          case 'unarchive':
            return { ...c, archived: false }
          case 'unread':
            return { ...c, unread_count: Math.max(1, c.unread_count) }
          case 'unstar':
            return { ...c, flagged: false }
        }
      })
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

export function useDeleteMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireDeleteThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) => (c.thread_id === threadId ? null : c))
      // Also strip any per-message Sent rows for this thread so a
      // delete triggered from the Sent view (or any view) removes the
      // row visibly instant. Refetch reconciles via invalidateMail().
      queryClient.setQueryData<readonly WireSentMessage[]>(mailKeys.sent(), (old) =>
        old ? old.filter((m) => m.thread_id !== threadId) : old
      )
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

// v2.4.1 Phase 3 (RFC-B §3.4) — move thread to Junk folder.
// Optimistic patch drops the thread from every currently-visible
// list; the Junk view repopulates on the next refetch (which the
// onSettled invalidateMail() call kicks off).
export function useMarkJunkMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireMarkJunk(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) => (c.thread_id === threadId ? null : c))
      return { snapshots }
    },
    onSettled: () => invalidateBucketMove(),
  })
}

// v2.4.1 Phase 3 (RFC-B §3.4) — move thread back to Inbox and
// auto-whitelist its senders on the backend. Same optimistic drop
// as `useMarkJunkMutation` — the Inbox view repopulates on the
// next refetch.
export function useMarkNotJunkMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireMarkNotJunk(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) => (c.thread_id === threadId ? null : c))
      return { snapshots }
    },
    onSettled: () => invalidateBucketMove(),
  })
}

// v2.9 triage — the three bucket-move mutations share the same
// optimistic-drop shape as junk: the moved thread vanishes from the
// current view and repopulates the target view on the next refetch.
function useBucketMoveMutation(mutationFn: (threadId: string) => Promise<void>) {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => mutationFn(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) => (c.thread_id === threadId ? null : c))
      return { snapshots }
    },
    onSettled: () => invalidateBucketMove(),
  })
}

export const useMarkNotificationMutation = () => useBucketMoveMutation(wireMarkNotification)
export const useMarkPromotionMutation = () => useBucketMoveMutation(wireMarkPromotion)
export const useMoveToInboxMutation = () => useBucketMoveMutation(wireMoveToInbox)

// ── send-side optimistic ─────────────────────────────────────────────
//
// Called by both new-conversation and reply-box after a successful
// sendMail. Prepends a placeholder onto the sent-messages cache so the
// row appears in the UI instantly, then invalidates the queries the
// server just updated (backend mirror_send_to_sender_view is
// synchronous — refetch swaps this uid=0 placeholder for the real row
// in one network RTT).
//
// One helper, two callers, on purpose: the "reply-box copies
// new-conversation's optimistic write" road ends with the two drifting
// out of sync the first time we tweak the placeholder shape
// (feedback-two-impls-need-a-contract-test).

export function applyOptimisticSent(msg: {
  message_id: string
  subject: string
  thread_id: string
  to: string
}): void {
  const placeholder: WireSentMessage = {
    internal_date: Math.floor(Date.now() / 1000),
    message_id: msg.message_id,
    subject: msg.subject,
    // uid=0 is temporary. Real rows have a real uid; the invalidate
    // below refetches server truth and swaps this out. SentList's
    // openMessage sets focusedMessageUid to msg.uid; a click on the
    // placeholder before refetch lands opens the thread but doesn't
    // scroll to a specific message — acceptable degradation for a
    // ~200 ms window.
    thread_id: msg.thread_id,
    to: msg.to,
    uid: 0,
  }
  queryClient.setQueryData<readonly WireSentMessage[]>(mailKeys.sent(), (old) =>
    dedupeSentByMessageId(placeholder, old)
  )
  void queryClient.invalidateQueries({ queryKey: mailKeys.sent() })
  void queryClient.invalidateQueries({ queryKey: mailKeys.conversations() })
  // A reply/forward drops a new message into a thread the user is
  // watching. Kick that thread's query too so the reply shows up in
  // the open timeline without waiting on the WS event or the 30 s
  // staleTime.
  if (msg.thread_id) {
    void queryClient.invalidateQueries({ queryKey: mailKeys.thread(msg.thread_id) })
  }
}

// ---- mark read / unread ----

export function useMarkReadMutation() {
  return useMutation<unknown, Error, { domains?: string[]; threadId: string }, Context>({
    mutationFn: ({ domains, threadId }) => wireMarkThreadRead(threadId, domains),
    onError: (_e, _vars, _ctx) => {
      // Do NOT rollback the optimistic patch on network / server error.
      // The retry path (auto-mark effect keyed on selectedUnreadCount)
      // would see the reverted unread > 0 and re-fire in a loop until
      // the network recovers — meanwhile the user sees the thread flip
      // back to unread even though they clearly opened it. Leaving the
      // patch in place gives the user Gmail-style visual continuity;
      // when connectivity returns, the next explicit action or a
      // WebSocket-driven refetch will reconcile with server truth.
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, unread_count: 0 } : c
      )
      // Keep this thread visible in the current 'unread' filter session
      // even though unread_count is now 0. Gmail-style: row only disappears
      // when the user re-enters the unread filter, never under their cursor.
      // No-op cost when the user isn't on the unread filter — the filter
      // predicate ignores the set unless quickFilter === 'unread'.
      addStickyUnread(threadId)
      return { snapshots }
    },
    // The optimistic patch IS the truth: server-side mark_thread_read writes
    // unread_count=0 and busts the kevy list cache; the client's optimistic
    // value matches server state byte-for-byte. Invalidating the conversations
    // query just forces a refetch that races against in-flight server
    // processing (between POST 200 and kevy bust + PG commit settle) and can
    // briefly overwrite the patch with stale list data, making the row flip
    // back to unread for ~100-500 ms — exactly the "mark-as-read doesn't
    // stick when I click fast" user complaint.
    // categories ARE server-computed aggregates that the client
    // cannot derive locally; they still need invalidation.
    onSettled: () => invalidateMailAggregatesOnly(),
  })
}

export function useMarkUnreadMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireMarkThreadUnread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, unread_count: Math.max(1, c.unread_count) } : c
      )
      // The row is genuinely unread again, no need to pin it as sticky any
      // longer — let the unread filter govern visibility on its own.
      removeStickyUnread(threadId)
      return { snapshots }
    },
    // Same as useMarkReadMutation: optimistic patch matches server state;
    // skip the conversations refetch that would race against in-flight
    // server processing.
    onSettled: () => invalidateMailAggregatesOnly(),
  })
}

// ---- star / unstar ----

export function usePinMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wirePinThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, pinned: true } : c
      )
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

export function useSnoozeMutation() {
  return useMutation<unknown, Error, { threadId: string; until: string }, Context>({
    mutationFn: ({ threadId, until }) => snoozeApi(threadId, until),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) => (c.thread_id === threadId ? null : c))
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

// ---- pin / unpin ----

export function useStarMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireStarThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, flagged: true } : c
      )
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

export function useUnarchiveMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireUnarchiveThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, archived: false } : c
      )
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

// ---- archive / unarchive ----

export function useUnpinMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireUnpinThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, pinned: false } : c
      )
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

export function useUnsnoozeMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => unsnoozeApi(threadId),
    onSettled: () => invalidateMail(),
  })
}

// ---- snooze (server returns success; we drop the row optimistically) ----

export function useUnstarMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => wireUnstarThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, flagged: false } : c
      )
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

function addStickyUnread(threadId: string) {
  const store = getDefaultStore()
  const next = new Set(store.get(stickyUnreadIdsAtom))
  next.add(threadId)
  store.set(stickyUnreadIdsAtom, next)
}

// ---- delete (single + batch share the same backend) ----

async function cancelConversationFetches() {
  // Cancel both the legacy key (still used by any not-yet-migrated
  // caller during Phase 3) AND the new v2.1 key that
  // `useConversationsQuery` moved onto.
  await Promise.all([
    queryClient.cancelQueries({ queryKey: mailKeys.conversations() }),
    queryClient.cancelQueries({ queryKey: conversationKeys.infinites() }),
    queryClient.cancelQueries({ queryKey: conversationKeys.lists() }),
  ])
}

// A bucket move (junk / not-junk / notification / promotion / inbox)
// changes WHICH list a thread belongs to, so the destination list — a
// different folder than the one on screen, therefore an INACTIVE query —
// must refetch too. The default `refetchType: 'active'` only refetches
// the mounted list, which is why the moved thread showed up in the
// target folder only after a hard refresh (2026-07-16). `'all'` refetches
// active + inactive conversation lists so switching to the target folder
// shows the thread immediately. Safe here: the backend has already moved
// the thread, so refetching the source list returns it correctly absent.
function invalidateBucketMove() {
  queryClient
    .invalidateQueries({ queryKey: conversationKeys.all(), refetchType: 'all' })
    .catch(() => {})
  queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
}

// ---- batch operations ----

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
function invalidateMail() {
  queryClient.invalidateQueries({ queryKey: mailKeys.conversations() }).catch(() => {})
  queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
  // Deleting/archiving a thread that has sent mail in it also removes
  // rows from the per-message Sent view — refetch so SentList doesn't
  // stale-display messages whose thread has already gone.
  queryClient.invalidateQueries({ queryKey: mailKeys.sent() }).catch(() => {})
  // v2.1 phase-3 — after the mail list migrated onto
  // `conversationKeys.infinite`, we broaden the invalidation to the
  // whole `conversation` entity namespace so both list + infinite
  // sub-caches refetch on the same trip. Cross-screen consistency
  // holds regardless of which screen a caller is on.
  queryClient.invalidateQueries({ queryKey: conversationKeys.all() }).catch(() => {})
}

// Invalidates only the small server-computed aggregate (categories) —
// leaves the conversations list cache alone. Used by mark-read /
// mark-unread, where the optimistic patch already matches what the
// server returns; a list refetch races against the post-POST
// processing window and can flip the row back to unread for 100-500 ms.
function invalidateMailAggregatesOnly() {
  queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
  // v2.1 phase-3 — cover the non-paginated `list` sub-namespace so
  // dashboard / sidebar aggregates recompute. The `infinite` cache is
  // left alone here (mark-read's optimistic patch already matches
  // server truth; a race-refetch would flicker rows back to unread).
  queryClient.invalidateQueries({ queryKey: conversationKeys.lists() }).catch(() => {})
}

function patchConversations(
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

function removeStickyUnread(threadId: string) {
  const store = getDefaultStore()
  const current = store.get(stickyUnreadIdsAtom)
  if (!current.has(threadId)) return
  const next = new Set(current)
  next.delete(threadId)
  store.set(stickyUnreadIdsAtom, next)
}

function rollbackConversations(snapshots: Array<[QueryKey, InfinitePages | undefined]>) {
  for (const [key, data] of snapshots) queryClient.setQueryData(key, data)
}
