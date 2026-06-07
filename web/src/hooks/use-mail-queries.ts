import type { CategoryCount, ConversationSummary, ThreadMessage } from '@/lib/types'

import { useInfiniteQuery, useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { mailKeys, type MailListFilters } from '@/lib/query-keys'
import { assertArrayShape } from '@/lib/runtime-shape'

const PAGE_SIZE = 50

const CONVERSATION_REQUIRED_KEYS = [
  'thread_id',
  'subject',
  'participants',
  'last_date',
  'unread_count',
] as const

const THREAD_MESSAGE_REQUIRED_KEYS = ['id', 'sender', 'subject', 'internal_date'] as const

export function useActionCountQuery(domains: string[]) {
  return useQuery({
    queryKey: mailKeys.actionCount(domains),
    staleTime: 60 * 1000,
    queryFn: ({ signal }) => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return fetchJson<{ count: number }>(`/conversations/action-count${q}`, signal)
    },
  })
}

export function useCategoriesQuery(domains: string[]) {
  return useQuery({
    queryKey: mailKeys.categories(domains),
    staleTime: 60 * 1000,
    queryFn: ({ signal }) => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return fetchJson<CategoryCount[]>(`/conversations/categories${q}`, signal)
    },
  })
}

// useInfiniteQuery so loadMore (older messages) lands as additional pages
// inside the same cache entry — refresh restores the whole stack, not just
// the first 50.
export function useConversationsQuery(filters: MailListFilters, enabled: boolean = true) {
  return useInfiniteQuery<
    ConversationSummary[],
    Error,
    { pageParams: (number | undefined)[]; pages: ConversationSummary[][] },
    ReturnType<typeof mailKeys.conversations>,
    number | undefined
  >({
    enabled,
    initialPageParam: undefined,
    queryKey: mailKeys.conversations(filters),
    getNextPageParam: (lastPage) => {
      if (lastPage.length < PAGE_SIZE) return undefined
      const last = lastPage[lastPage.length - 1]
      return last?.last_date
    },
    queryFn: ({ pageParam, signal }) =>
      fetchJson<ConversationSummary[]>(listPath(filters, pageParam), signal, (raw) =>
        assertArrayShape<ConversationSummary>('/conversations', raw, CONVERSATION_REQUIRED_KEYS)
      ),
  })
}

export function useThreadQuery(threadId: null | string, domains: string[]) {
  return useQuery({
    enabled: !!threadId,
    queryFn: ({ signal }) => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return fetchJson<ThreadMessage[]>(
        `/conversations/${encodeURIComponent(threadId ?? '')}${q}`,
        signal,
        (raw) =>
          assertArrayShape<ThreadMessage>(
            '/conversations/:threadId',
            raw,
            THREAD_MESSAGE_REQUIRED_KEYS
          )
      )
    },
    // Thread content is mutation-invariant from the client's point of view —
    // mark-read / star / pin / archive all act on list-shape flags only, not
    // on the message body / attachments / headers. The only thing that can
    // change a thread's content is an inbound message landing on that thread,
    // and that flows through the NewMessage WebSocket event in
    // use-mail-events.ts which explicitly invalidates this query.
    // staleTime: Infinity here means clicking back to a previously-opened
    // thread renders instantly from cache, no refetch, no spinner.
    queryKey: mailKeys.thread(threadId),
    staleTime: Infinity,
  })
}

// Build the API path for a paginated conversation list. Mirrors the old
// chat.tsx `buildPath` but pure — no React state.
function listPath(filters: MailListFilters, before?: number): string {
  if (filters.query) {
    let p = `/conversations/search?q=${encodeURIComponent(filters.query)}&limit=${PAGE_SIZE}`
    if (filters.category) p += `&category=${encodeURIComponent(filters.category)}`
    if (filters.domains && filters.domains.length > 0) {
      p += `&domains=${encodeURIComponent(filters.domains.join(','))}`
    }
    return p
  }
  let p = `/conversations?limit=${PAGE_SIZE}`
  if (before) p += `&before=${before}`
  if (filters.category) p += `&category=${encodeURIComponent(filters.category)}`
  if (filters.domains && filters.domains.length > 0) {
    p += `&domains=${encodeURIComponent(filters.domains.join(','))}`
  }
  if (filters.archived) p += '&archived=true'
  if (filters.folder) p += `&folder=${encodeURIComponent(filters.folder)}`
  if (filters.unread) p += '&unread=true'
  if (filters.starred) p += '&starred=true'
  if (filters.section) p += `&section=${encodeURIComponent(filters.section)}`
  return p
}
