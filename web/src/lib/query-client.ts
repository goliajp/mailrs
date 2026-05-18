import { createSyncStoragePersister } from '@tanstack/query-sync-storage-persister'
import { QueryClient } from '@tanstack/react-query'

// Single shared QueryClient. Imported by main.tsx (for the
// PersistQueryClientProvider) and by use-mail-events.ts (for imperative
// invalidateQueries / setQueryData calls outside the React tree).
//
// Defaults bias toward "show cached immediately, refetch quietly":
//   - staleTime 30s: a fresh fetch is considered fresh for half a minute
//     before triggering a background refetch on remount / focus.
//   - gcTime 30min: keep unused queries in memory for half an hour so
//     back-button / tab-switch doesn't re-fetch.
//   - refetchOnWindowFocus false: we drive freshness via WebSocket
//     invalidation; window focus shouldn't thunder-herd.
//   - retry 1: most failures are transient or auth; loud failure is better
//     than silently retrying forever.
export const queryClient = new QueryClient({
  defaultOptions: {
    mutations: {
      retry: 0,
    },
    queries: {
      gcTime: 30 * 60 * 1000,
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 30 * 1000,
    },
  },
})

// Build buster so a deploy of incompatible cache shape doesn't blow up on
// users with old localStorage. Vite injects __APP_BUILD_ID__ at build time;
// in dev / tests this is undefined → we fall back to a constant.
declare const __APP_BUILD_ID__: string | undefined
const buildId = typeof __APP_BUILD_ID__ !== 'undefined' ? __APP_BUILD_ID__ : 'dev'

export const persister = createSyncStoragePersister({
  key: `mailrs:rq:v1:${buildId}`,
  storage: typeof window !== 'undefined' ? window.localStorage : undefined,
  // Be polite: skip persisting queries that are loading or errored; only
  // settled (success) entries are worth restoring on next boot.
  throttleTime: 1000,
})
