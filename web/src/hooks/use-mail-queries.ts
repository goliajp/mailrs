import type { CategoryCount, ConversationSummary, ThreadMessage } from '@/lib/types'

import { queryOptions, useInfiniteQuery, useQuery } from '@tanstack/react-query'

import { conversationListKey, LIST_PAGE_SIZE, listPath } from '@/lib/list-axes'
import { mailKeys, type MailListFilters } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'
import { wireFetch } from '@/wire/client'
import { adminListGet } from '@/wire/endpoints/admin'
import {
  wireThreadDetailResponseSchema,
  wireThreadListResponseSchema,
} from '@/wire/schemas/conversation'

/**
 * The thread query without its `enabled` gate, so a prefetch fills the
 * very cache entry the reading pane reads — same key, same fetch, same
 * freshness.
 */
export function threadQueryOptions(threadId: null | string, domains: string[]) {
  return queryOptions({
    queryFn: async ({ signal }) => {
      // v2.1 §7 (2026-07-08): Zod-parse the wire response.
      // wireThreadDetailResponseSchema accepts both envelope shapes.
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      const parsed = await wireFetch(wireThreadDetailResponseSchema, {
        path: `/conversations/${encodeURIComponent(threadId ?? '')}${q}`,
        signal,
      })
      return parsed.items as unknown as ThreadMessage[]
    },
    // Thread content is mutation-invariant from the client's point of
    // view — mark-read / star / pin / archive act on list-shape flags
    // only, not on the body, attachments or headers. What changes it
    // is an inbound message landing on the thread, and that reaches a
    // *connected* tab through the NewMessage WebSocket event in
    // use-mail-events.ts, which invalidates this query.
    //
    // It used to say that was the only way, and set `staleTime:
    // Infinity` on the strength of it. A closed, sleeping or offline
    // tab misses the event — they live five minutes — and the cache
    // is persisted, so the stale entry came back on every reload. See
    // the `staleTime` below.
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
    // **Not `Infinity`.** The comment above says the only thing that
    // can change a thread's content is an inbound message arriving on
    // it, and that this flows through the WebSocket — which is true
    // while the tab is open and connected, and false the rest of the
    // time. Events live five minutes in kevy; a tab that was closed,
    // asleep or offline misses them.
    //
    // With `Infinity` that is not a five-minute gap but a permanent
    // one, because the cache is persisted to localStorage and keyed on
    // the build: a reload restores the stale entry and never refetches
    // it. Reported 2026-09-01 — the list row carried the newest
    // message's timestamp while the conversation pane showed two of
    // three, and reloading did not fix it.
    //
    // A minute is long enough to keep a back-and-forth between two
    // threads instant, which is what `Infinity` was for, and short
    // enough that a reload is never wrong.
    staleTime: 60_000,
    placeholderData: () => undefined,
  })
}

export function useCategoriesQuery(domains: string[]) {
  return useQuery({
    queryKey: mailKeys.categories(domains),
    staleTime: 60 * 1000,
    queryFn: ({ signal }) => {
      const q = domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
      return adminListGet<CategoryCount>(`/conversations/categories${q}`, signal)
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
    { pageParams: (string | undefined)[]; pages: ConversationSummary[][] },
    ReturnType<typeof conversationKeys.infinite>,
    string | undefined
  >({
    enabled,
    initialPageParam: undefined,
    queryKey: conversationListKey(filters),
    getNextPageParam: (lastPage) => {
      if (lastPage.length < LIST_PAGE_SIZE) return undefined
      // search answers in one page; asking it again appends the same rows
      if (filters.query) return undefined
      // the server's place after the last row: the time it is ordered by
      // (the waiting list's own time included) and a tie-breaker, so rows
      // sharing that second are neither skipped nor shown twice
      const cursor = lastPage[lastPage.length - 1]?.cursor
      if (!cursor) return undefined
      return cursor
    },
    queryFn: async ({ pageParam, signal }) => {
      // v2.1 §7 (2026-07-08): Zod-parse the wire response.
      // wireThreadListResponseSchema accepts both envelope shapes
      // (`{items: [...]}` and bare array), so this is a drop-in for
      // `adminListGet` — just adds shape validation at the boundary.
      const parsed = await wireFetch(wireThreadListResponseSchema, {
        path: listPath(filters, pageParam),
        signal,
      })
      // Cast is safe: wire schema has strict subset guarantee that
      // downstream `ConversationSummary` needs. TODO §D: migrate to
      // domain `ThreadSummary` shape and drop the cast.
      return parsed.items as unknown as ConversationSummary[]
    },
    // The one query where being wrong is visible at a glance, so it
    // gets the safety net the global default gives up.
    //
    // `refetchOnWindowFocus` is false for every query (see
    // `query-client.ts`): freshness is meant to arrive over the
    // WebSocket. That holds for anything this app does to itself, and
    // not at all for a change it did not make — another device, the
    // iOS client, or a server-side repair, none of which emit an event
    // this tab will hear. On 2026-08-12 a maintenance sweep corrected
    // seven conversations' dates and the open tab went on showing the
    // old order indefinitely: the server was right, the list was
    // wrong, and nothing short of a reload would say so.
    //
    // Not a thundering herd: `staleTime` is 30s, so a focus only
    // refetches a list that is already stale, and only this one.
    refetchOnReconnect: true,
    refetchOnWindowFocus: true,
  })
}

export function useThreadQuery(threadId: null | string, domains: string[]) {
  return useQuery({ enabled: !!threadId, ...threadQueryOptions(threadId, domains) })
}
