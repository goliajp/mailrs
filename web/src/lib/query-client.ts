import { createSyncStoragePersister } from '@tanstack/query-sync-storage-persister'
import { keepPreviousData, QueryClient } from '@tanstack/react-query'

// Single shared QueryClient. Imported by main.tsx (for the
// PersistQueryClientProvider) and by use-mail-events.ts (for imperative
// invalidateQueries / setQueryData calls outside the React tree).
//
// v2.1 defaults — see RFC §2.4, §3.5:
//   - staleTime 30s: a fresh fetch is considered fresh for half a
//     minute before triggering a background refetch on remount /
//     focus. Route transitions within staleTime are instant, no fetch.
//   - gcTime 30min: keep unused queries in memory for half an hour so
//     back-button / tab-switch doesn't re-fetch.
//   - placeholderData: keepPreviousData — filter changes and
//     pagination NEVER blank the screen. RQ keeps the previous
//     resolved value on-screen until the new query lands. This is
//     the anti-flash discipline the RFC requires.
//   - refetchOnWindowFocus false: we drive freshness via WebSocket
//     invalidation; window focus shouldn't thunder-herd.
//   - retry 1: most failures are transient or auth; loud failure is
//     better than silently retrying forever.
export const queryClient = new QueryClient({
  defaultOptions: {
    mutations: {
      retry: 0,
    },
    queries: {
      gcTime: 30 * 60 * 1000,
      placeholderData: keepPreviousData,
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

/**
 * One key, forever — the build id is the *buster*, not part of the name.
 *
 * It used to be `mailrs:rq:v1:${buildId}`, which changed on every
 * deploy and left the previous snapshot behind: nothing ever deleted a
 * key it no longer used. Each release added another full copy of the
 * query cache until localStorage hit its quota, and then the next
 * `localStorage.setItem` threw — including the one in `saveAuth`, so
 * **signing in stopped working** and the login page said "Network
 * error" because that is what its catch-all says. Reported
 * 2026-08-11; the login POST answered 200 the whole time.
 *
 * `PersistQueryClientProvider` compares the buster it is given against
 * the one in the stored blob and throws the blob away when they
 * differ. That is the same protection the old name gave, minus the
 * leak.
 */
export const PERSIST_KEY = 'mailrs:rq:v1'

export const persister = createSyncStoragePersister({
  key: PERSIST_KEY,
  storage: typeof window !== 'undefined' ? window.localStorage : undefined,
  // Be polite: skip persisting queries that are loading or errored; only
  // settled (success) entries are worth restoring on next boot.
  throttleTime: 1000,
})

/** The buster: a new build discards the previous build's cache. */
export const persistBuster = buildId

/**
 * Delete the snapshots earlier builds left behind.
 *
 * Every deploy before 2026-08-12 wrote to a key of its own and removed
 * nothing, so a long-lived browser holds one blob per release. Run
 * once at boot: without it the quota stays full and the fix above
 * changes nothing for the people it already broke.
 */
export function dropOrphanedCaches(): number {
  if (typeof window === 'undefined') return 0
  const stale = Object.keys(window.localStorage).filter(
    (k) => k.startsWith('mailrs:rq:') && k !== PERSIST_KEY
  )
  for (const key of stale) window.localStorage.removeItem(key)
  return stale.length
}
