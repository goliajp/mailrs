import type { CategoryCount, ConversationSummary, ThreadMessage } from '@/lib/types'

import { useInfiniteQuery, useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { mailKeys, type MailListFilters } from '@/lib/query-keys'

const PAGE_SIZE = 50

export function useActionCountQuery(domains: string[]) {
  return useQuery({
    queryKey: mailKeys.actionCount(domains),
    staleTime: 60 * 1000,
    queryFn: () => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return fetchJson<{ count: number }>(`/conversations/action-count${q}`)
    },
  })
}

export function useCategoriesQuery(domains: string[]) {
  return useQuery({
    queryKey: mailKeys.categories(domains),
    staleTime: 60 * 1000,
    queryFn: () => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return fetchJson<CategoryCount[]>(`/conversations/categories${q}`)
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
    queryFn: ({ pageParam }) => fetchJson<ConversationSummary[]>(listPath(filters, pageParam)),
  })
}

export function useThreadQuery(threadId: null | string, domains: string[]) {
  return useQuery({
    enabled: !!threadId,
    queryFn: () => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return fetchJson<ThreadMessage[]>(`/conversations/${encodeURIComponent(threadId ?? '')}${q}`)
    },
    // threads change less often than the list; longer fresh window cuts
    // chatter when user clicks between the same few threads repeatedly.
    queryKey: mailKeys.thread(threadId),
    staleTime: 5 * 60 * 1000,
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
