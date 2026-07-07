/**
 * `useConversationList` / `useUnreadCount` — layer 4 façade on
 * `conversationKeys` (see RFC §2.4, §2.5).
 *
 * Every screen reads through these. Nobody accesses
 * `queryClient.getQueryData` for conversation data directly.
 */

import type { ConversationFilter, ThreadId, ThreadSummary } from '@/domain'

import { useQuery, useQueryClient, type UseQueryResult } from '@tanstack/react-query'
import { useCallback, useMemo } from 'react'

import * as commands from '@/reducers/commands/conversation'
import { conversationKeys } from '@/store/query-keys-v21'
import { fetchConversationList } from '@/wire/endpoints/conversations'

export type ConversationListResult = UseQueryResult<{ readonly items: readonly ThreadSummary[] }>

/**
 * Commands surface — every screen dispatches through this, never
 * touches the query client directly. Each command handler encapsulates
 * snapshot / optimistic patch / wire / reconcile / rollback.
 */
export function useConversationCommands() {
  const qc = useQueryClient()
  const markRead = useCallback(
    (threadId: ThreadId, domains?: readonly string[]) =>
      commands.markThreadRead(qc, threadId, domains),
    [qc]
  )
  const markUnread = useCallback(
    (threadId: ThreadId, domains?: readonly string[]) =>
      commands.markThreadUnread(qc, threadId, domains),
    [qc]
  )
  const setStarred = useCallback(
    (threadId: ThreadId, starred: boolean) => commands.starThread(qc, threadId, starred),
    [qc]
  )
  return { markRead, markUnread, setStarred }
}

/**
 * The one true conversation-list reader. Every screen — dashboard,
 * mail list, sidebar badge — subscribes here. React Query dedupes on
 * the canonicalised filter, so N concurrent callers share one fetch
 * and one cache line.
 */
export function useConversationList(filter: ConversationFilter = {}): ConversationListResult {
  return useQuery({
    queryKey: conversationKeys.list(filter),
    queryFn: ({ signal }) => fetchConversationList(filter, signal),
    // 30 s freshness across route transitions — matches RFC §2.4
    // "cache lifetime governed by two global defaults." Overridden
    // per-caller only with a written rationale.
    staleTime: 30_000,
    // Never blank the screen on a filter change; keep the previous
    // rows visible while the new query runs.
    placeholderData: (prev) => prev,
  })
}

/**
 * Unread count is a pure derivation on `useConversationList({folder:
 * 'INBOX'})`. When any command patches the underlying list, this
 * selector recomputes on the next render — no separate cache to keep
 * in sync.
 */
export function useUnreadCount(): number {
  const { data } = useConversationList({ folder: 'INBOX' })
  return useMemo(() => (data?.items ?? []).reduce((sum, t) => sum + t.unreadCount, 0), [data])
}
