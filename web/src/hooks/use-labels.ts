// The signed-in user's labels, and putting them on conversations. Putting
// one on writes the row's `labels` and lets `belongsTo` place it, as the
// other mail mutations do.

import type { ConversationSummary } from '@/lib/types'

import { useMutation, useQuery } from '@tanstack/react-query'

import {
  cancelConversationFetches,
  invalidateMail,
  type MutationContext,
  patchConversationFields,
  rollbackConversations,
} from '@/lib/mail-cache'
import { queryClient } from '@/lib/query-client'
import {
  wireCreateLabel,
  wireDeleteLabel,
  wireListLabels,
  wireSetThreadLabel,
  wireUpdateLabel,
} from '@/wire/endpoints/labels'

export const labelsKey = ['labels'] as const

type SetLabel = { id: string; on: boolean; threadIds: string[] }

export function useCreateLabel() {
  return useMutation({
    onSettled: refreshLabels,
    mutationFn: ({ color, name }: { color: string; name: string }) => wireCreateLabel(name, color),
  })
}

/** Deleting a label takes it off every row; the rows stay. */
export function useDeleteLabel() {
  return useMutation({
    mutationFn: (id: string) => wireDeleteLabel(id),
    onSettled: () => {
      refreshLabels()
      invalidateMail()
    },
  })
}

export function useLabels() {
  return useQuery({
    queryKey: labelsKey,
    queryFn: ({ signal }) => wireListLabels(signal),
  })
}

export function useSetThreadLabel() {
  return useMutation<unknown, Error, SetLabel, MutationContext>({
    mutationFn: async ({ id, on, threadIds }) => {
      for (const tid of threadIds) await wireSetThreadLabel(tid, id, on)
    },
    onError: (_e, _vars, ctx) => {
      if (ctx) rollbackConversations(ctx.snapshots)
    },
    onMutate: async ({ id, on, threadIds }) => {
      await cancelConversationFetches()
      const set = new Set(threadIds)
      const snapshots = patchConversationFields((c) => withLabel(c, set, id, on))
      return { snapshots }
    },
    onSettled: () => invalidateMail(),
  })
}

export function useUpdateLabel() {
  return useMutation({
    onSettled: refreshLabels,
    mutationFn: ({ change, id }: { change: { color?: string; name?: string }; id: string }) =>
      wireUpdateLabel(id, change),
  })
}

function refreshLabels() {
  void queryClient.invalidateQueries({ queryKey: labelsKey })
}

function withLabel(
  c: ConversationSummary,
  threads: Set<string>,
  id: string,
  on: boolean
): ConversationSummary {
  if (!threads.has(c.thread_id)) return c
  const rest = (c.labels ?? []).filter((l) => l !== id)
  if (on) return { ...c, labels: [...rest, id] }
  return { ...c, labels: rest }
}
