import type { CategoryCount, ConversationSummary, ThreadMessage } from '@/lib/types'

import { useInfiniteQuery, useQuery } from '@tanstack/react-query'

import { fetchJson, fetchList } from '@/lib/api'
import { mailKeys, type MailListFilters } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'

const PAGE_SIZE = 50

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
      return fetchList<CategoryCount>(`/conversations/categories${q}`, signal)
    },
  })
}

// useInfiniteQuery so loadMore (older messages) lands as additional pages
// inside the same cache entry — refresh restores the whole stack, not just
// the first 50.
//
// v2.1 phase-3 migration: the queryKey is now the entity-oriented
// `conversationKeys.infinite(filter)` — one cache-line per filter,
// scoped under the `conversation.infinite` namespace so the dashboard's
// `list` reads (see `pages/dashboard.tsx`) don't collide with the
// scroll state. Bridge invalidation in `use-mail-mutations` covers
// both sub-namespaces via `conversationKeys.all()`.
export function useConversationsQuery(filters: MailListFilters, enabled: boolean = true) {
  return useInfiniteQuery<
    ConversationSummary[],
    Error,
    { pageParams: (number | undefined)[]; pages: ConversationSummary[][] },
    ReturnType<typeof conversationKeys.infinite>,
    number | undefined
  >({
    enabled,
    initialPageParam: undefined,
    queryKey: conversationKeys.infinite({
      archived: filters.archived,
      category: filters.category as never,
      domains: filters.domains,
      folder: filters.folder as never,
      starred: filters.starred,
      unread: filters.unread,
    }),
    getNextPageParam: (lastPage) => {
      if (lastPage.length < PAGE_SIZE) return undefined
      const last = lastPage[lastPage.length - 1]
      return last?.last_date
    },
    queryFn: ({ pageParam, signal }) =>
      fetchList<ConversationSummary>(listPath(filters, pageParam), signal),
  })
}

export function useThreadQuery(threadId: null | string, domains: string[]) {
  return useQuery({
    enabled: !!threadId,
    queryFn: ({ signal }) => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return fetchList<ThreadMessage>(
        `/conversations/${encodeURIComponent(threadId ?? '')}${q}`,
        signal
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
    // v2.1 phase-6 anti-flash defaults set `placeholderData: keepPreviousData`
    // globally so mail-list filter changes never blank the screen. That's
    // wrong for a per-thread query: on a thread switch we WANT
    // `data === undefined` until the new thread resolves, so ThreadView's
    // bridge effect can clear the previous thread's messages instead of
    // mistakenly attributing them to the new thread. Setting the option
    // to `undefined` here does NOT override the global default (RQ 5 reads
    // `undefined` as "not specified" and falls through) — the correct
    // opt-out is a function that returns undefined for any prior data.
    // Without this opt-out, thread A's messages leak into thread B's
    // timeline during the fetch window, and every A→B→A round-trip
    // append a stale bubble (2026-07-08 user report of 5 duplicate "Me"
    // rows accumulating after repeated clicks).
    queryKey: mailKeys.thread(threadId),
    staleTime: Infinity,
    placeholderData: () => undefined,
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
