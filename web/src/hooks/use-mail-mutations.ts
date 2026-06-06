import type { ConversationSummary } from '@/lib/types'

import { type QueryKey, useMutation } from '@tanstack/react-query'

import {
  deleteJson,
  postJson,
  snoozeConversation as snoozeApi,
  unsnoozeConversation as unsnoozeApi,
} from '@/lib/api'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'

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

export type BatchAction = 'archive' | 'delete' | 'read' | 'star' | 'unarchive' | 'unread' | 'unstar'

type BatchResult = {
  failed: number
  message?: string
  processed: number
  success: boolean
}

type Context = { snapshots: Array<[QueryKey, InfinitePages | undefined]> }

type InfinitePages = { pageParams: (number | undefined)[]; pages: ConversationSummary[][] }

export function useArchiveMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) =>
      postJson(`/conversations/${encodeURIComponent(threadId)}/archive`, {}),
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
    onSettled: () => invalidateMail(),
  })
}

export function useBatchMutation() {
  return useMutation<BatchResult, Error, { action: BatchAction; threadIds: string[] }, Context>({
    mutationFn: ({ action, threadIds }) =>
      postJson<BatchResult>('/conversations/batch', { action, thread_ids: threadIds }),
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

// ---- mark read / unread ----

export function useDeleteMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => deleteJson(`/conversations/${encodeURIComponent(threadId)}`),
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

export function useMarkReadMutation() {
  return useMutation<unknown, Error, { domains?: string[]; threadId: string }, Context>({
    mutationFn: ({ domains, threadId }) => {
      const q =
        domains && domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return postJson(`/conversations/${encodeURIComponent(threadId)}/read${q}`, {})
    },
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, unread_count: 0 } : c
      )
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
    // categories / actionCount ARE server-computed aggregates that the client
    // cannot derive locally; they still need invalidation.
    onSettled: () => invalidateMailAggregatesOnly(),
  })
}

// ---- star / unstar ----

export function useMarkUnreadMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) =>
      postJson(`/conversations/${encodeURIComponent(threadId)}/unread`, {}),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversations((c) =>
        c.thread_id === threadId ? { ...c, unread_count: Math.max(1, c.unread_count) } : c
      )
      return { snapshots }
    },
    // Same as useMarkReadMutation: optimistic patch matches server state;
    // skip the conversations refetch that would race against in-flight
    // server processing.
    onSettled: () => invalidateMailAggregatesOnly(),
  })
}

export function usePinMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) =>
      postJson(`/conversations/${encodeURIComponent(threadId)}/pin`, {}),
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

// ---- pin / unpin ----

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

export function useStarMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) =>
      postJson(`/conversations/${encodeURIComponent(threadId)}/star`, {}),
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

// ---- archive / unarchive ----

export function useUnarchiveMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) =>
      postJson(`/conversations/${encodeURIComponent(threadId)}/unarchive`, {}),
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

export function useUnpinMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) =>
      postJson(`/conversations/${encodeURIComponent(threadId)}/unpin`, {}),
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

// ---- snooze (server returns success; we drop the row optimistically) ----

export function useUnsnoozeMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) => unsnoozeApi(threadId),
    onSettled: () => invalidateMail(),
  })
}

export function useUnstarMutation() {
  return useMutation<unknown, Error, { threadId: string }, Context>({
    mutationFn: ({ threadId }) =>
      postJson(`/conversations/${encodeURIComponent(threadId)}/unstar`, {}),
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

// ---- delete (single + batch share the same backend) ----

async function cancelConversationFetches() {
  await queryClient.cancelQueries({ queryKey: mailKeys.conversations() })
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
  queryClient.invalidateQueries({ queryKey: mailKeys.actionCount([]) }).catch(() => {})
}

// Invalidates only the small server-computed aggregates (categories +
// actionCount) — leaves the conversations list cache alone. Used by
// mark-read / mark-unread, where the optimistic patch already matches
// what the server returns; a list refetch races against the post-POST
// processing window and can flip the row back to unread for 100-500 ms.
function invalidateMailAggregatesOnly() {
  queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
  queryClient.invalidateQueries({ queryKey: mailKeys.actionCount([]) }).catch(() => {})
}

function patchConversations(
  patch: (c: ConversationSummary) => ConversationSummary | null
): Array<[QueryKey, InfinitePages | undefined]> {
  // snapshot before any writes so onError can revert exactly what we mutated
  const snapshots = queryClient.getQueriesData<InfinitePages>({
    queryKey: mailKeys.conversations(),
  })
  queryClient.setQueriesData<InfinitePages>({ queryKey: mailKeys.conversations() }, (old) => {
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
  })
  return snapshots
}

function rollbackConversations(snapshots: Array<[QueryKey, InfinitePages | undefined]>) {
  for (const [key, data] of snapshots) queryClient.setQueryData(key, data)
}
