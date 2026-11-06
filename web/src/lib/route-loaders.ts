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
// The list on screen is decided by atoms (`activeListAtom` and the
// refinements beside it), not by the URL. The mail loader reads the same
// atoms through the same `mailFiltersOf`, and builds the path and the
// cache key with the list's own `listPath` and `conversationListKey`, so
// the page it primes is the page the list reads. It used to read
// `?folder=` / `?tab=` from the URL, which nothing writes, and primed a
// whole-mailbox page under a key no list used.

import type { CategoryCount } from '@/lib/types'

import { getDefaultStore } from 'jotai'

import { conversationListKey, listPath } from '@/lib/list-axes'
import { mailFilterInputsIn, mailFiltersOf } from '@/lib/mail-filters'
import { queryClient } from '@/lib/query-client'
import { mailKeys, type MailListFilters } from '@/lib/query-keys'
import { wireFetch } from '@/wire/client'
import { adminListGet } from '@/wire/endpoints/admin'
import { wireThreadListResponseSchema } from '@/wire/schemas/conversation'

// Dashboard loader — same idea but for INBOX list + categories. The
// action-count query was removed in Phase 1.1, so the dashboard only
// needs these two. Runs both fetches concurrently.
export async function dashboardLoader({ request: _request }: { request: Request }) {
  const filters: MailListFilters = {
    archived: false,
    category: null,
    domains: [],
    folder: 'INBOX',
    section: null,
    starred: false,
    unread: false,
  }
  primeList(filters)
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
export async function mailListLoader({ request: _request }: { request: Request }) {
  const filters = mailFiltersOf(mailFilterInputsIn(getDefaultStore()))
  // Send and Draft are not served by `/conversations`
  if (!filters) return null
  primeList(filters)
  return null
}

/** Start the first page of `filters`' list, under the key the list reads. */
function primeList(filters: MailListFilters) {
  queryClient
    .ensureInfiniteQueryData({
      initialPageParam: undefined as string | undefined,
      queryKey: conversationListKey(filters),
      queryFn: async ({ pageParam, signal }) => {
        const parsed = await wireFetch(wireThreadListResponseSchema, {
          path: listPath(filters, pageParam as string | undefined),
          signal,
        })
        return parsed.items
      },
    })
    .catch(() => {})
}
