// Route-level data loaders (v2.3.3 roadmap Phase 1.2).
//
// react-router v7 data-router loaders that queue the primary read
// queries for a route before its React tree mounts. The observable
// win: `useConversationsQuery` etc. no longer wait for their component
// to render — the fetch is already in-flight by the time the child
// mounts, so the very first paint gets to hit RQ cache and skip the
// initial spinner.
//
// Loaders don't return data — they just kick off `ensureQueryData`,
// which caches the promise so the child hook's queryKey lookup finds
// an in-flight (or resolved) promise instead of undefined.
//
// URL params are the source of truth for filters (D11). The loader
// reads them and derives the same filter shape the component would
// have computed via atoms — so on refresh, the URL alone re-primes
// the cache. Atoms remain the transient UI state (hover, selection)
// but stop being the primary "which folder am I in" state.

import type { CategoryCount } from '@/lib/types'

import { queryClient } from '@/lib/query-client'
import { mailKeys, type MailListFilters } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'
import { wireFetch } from '@/wire/client'
import { adminListGet } from '@/wire/endpoints/admin'
import { wireThreadListResponseSchema } from '@/wire/schemas/conversation'

const PAGE_SIZE = 50

// Dashboard loader — same idea but for INBOX list + categories. The
// action-count query was removed in Phase 1.1, so the dashboard only
// needs these two. Runs both fetches concurrently.
export async function dashboardLoader({ request: _request }: { request: Request }) {
  const filters: MailListFilters = {
    archived: false,
    category: null,
    domains: [],
    folder: 'INBOX',
    query: '',
    section: null,
    starred: false,
    unread: false,
  }
  queryClient
    .ensureInfiniteQueryData({
      initialPageParam: undefined as number | undefined,
      queryKey: conversationKeys.infinite({
        archived: false,
        category: null as never,
        domains: [],
        folder: 'INBOX' as never,
        starred: false,
        unread: false,
      }),
      queryFn: async ({ pageParam, signal }) => {
        const parsed = await wireFetch(wireThreadListResponseSchema, {
          path: listPath(filters, pageParam as number | undefined),
          signal,
        })
        return parsed.items
      },
    })
    .catch(() => {})
  queryClient
    .ensureQueryData({
      queryKey: mailKeys.categories([]),
      queryFn: ({ signal }) => adminListGet<CategoryCount>('/conversations/categories', signal),
    })
    .catch(() => {})
  return null
}

// react-router v7 loader signature. Router calls this before the
// route's `element` renders; we prime RQ and return null. The RQ
// hooks inside the component then find an in-flight or resolved
// promise for the same queryKey and skip the initial fetch.
export async function mailListLoader({ request }: { request: Request }) {
  const url = new URL(request.url)
  const filters = filtersFromUrl(url)
  const key = conversationKeys.infinite({
    archived: filters.archived,
    category: filters.category as never,
    domains: filters.domains,
    folder: filters.folder as never,
    starred: filters.starred,
    unread: filters.unread,
  })
  queryClient
    .ensureInfiniteQueryData({
      initialPageParam: undefined as number | undefined,
      queryKey: key,
      queryFn: async ({ pageParam, signal }) => {
        const parsed = await wireFetch(wireThreadListResponseSchema, {
          path: listPath(filters, pageParam as number | undefined),
          signal,
        })
        return parsed.items
      },
    })
    .catch(() => {})
  return null
}

function filtersFromUrl(url: URL): MailListFilters {
  const folder = url.searchParams.get('folder') ?? null
  const tab = url.searchParams.get('tab')
  const cat = url.searchParams.get('cat') ?? null
  return {
    archived: false,
    category: cat,
    domains: [],
    folder,
    query: '',
    section: null,
    starred: tab === 'starred',
    unread: tab === 'unread',
  }
}

// Mirrors `use-mail-queries.ts::listPath`. Kept in sync by hand — the
// loader would otherwise import from the hook module, which triggers
// a chain that pulls in React and blocks the router bundle from
// pre-loading.
function listPath(filters: MailListFilters, before?: number): string {
  const params = new URLSearchParams()
  params.set('limit', String(PAGE_SIZE))
  if (before !== undefined) params.set('before', String(before))
  if (filters.folder) params.set('folder', filters.folder)
  if (filters.category) params.set('category', filters.category)
  if (filters.unread) params.set('unread', '1')
  if (filters.starred) params.set('starred', '1')
  if (filters.archived) params.set('archived', '1')
  if (filters.query) params.set('q', filters.query)
  if (filters.domains && filters.domains.length > 0) {
    params.set('domains', filters.domains.join(','))
  }
  return `/conversations?${params.toString()}`
}
