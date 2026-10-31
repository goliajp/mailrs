// The Trash's mutations. Moving to the Trash and back writes `trashed_at`
// and lets `belongsTo` place the row; deleting for good and emptying drop
// rows outright.

import type { ConversationSummary } from '@/lib/types'
import type { WireSentMessage } from '@/wire/schemas/mail'

import { useMutation } from '@tanstack/react-query'

import {
  cancelConversationFetches,
  invalidateMail,
  type MutationContext,
  patchConversationFields,
  patchConversations,
  rollbackConversations,
} from '@/lib/mail-cache'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import {
  wireDeleteThreadForever,
  wireEmptyTrash,
  wireTrashThread,
  wireUntrashThread,
} from '@/wire/endpoints/mutations'

/** For good: the row goes from every list. */
export function useDeleteForeverMutation() {
  return useMutation<unknown, Error, { threadId: string }, MutationContext>({
    mutationFn: ({ threadId }) => wireDeleteThreadForever(threadId),
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

/** Everything in the Trash, for good. */
export function useEmptyTrashMutation() {
  return useMutation<{ deleted: number }, Error, void, MutationContext>({
    mutationFn: () => wireEmptyTrash(),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async () => {
      await cancelConversationFetches()
      const snapshots = patchConversations(keepUnlessTrashed)
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

/** To the Trash: the row leaves its list and joins the Trash. */
export function useTrashMutation() {
  return useMutation<unknown, Error, { threadId: string }, MutationContext>({
    mutationFn: ({ threadId }) => wireTrashThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const now = Math.floor(Date.now() / 1000)
      const snapshots = patchConversationFields((c) => withTrashedAt(c, threadId, now))
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

/** Out of the Trash, back to the list it was in. */
export function useUntrashMutation() {
  return useMutation<unknown, Error, { threadId: string }, MutationContext>({
    mutationFn: ({ threadId }) => wireUntrashThread(threadId),
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ threadId }) => {
      await cancelConversationFetches()
      const snapshots = patchConversationFields((c) => withTrashedAt(c, threadId, 0))
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

/** Emptying the Trash: every row in it goes. */
function keepUnlessTrashed(c: ConversationSummary): ConversationSummary | null {
  if ((c.trashed_at ?? 0) > 0) return null
  return c
}

/** The row with `trashed_at` set when it is `threadId`'s, else as it was. */
function withTrashedAt(c: ConversationSummary, threadId: string, at: number): ConversationSummary {
  if (c.thread_id !== threadId) return c
  return { ...c, trashed_at: at }
}
