/**
 * Snapshot / rollback helpers — layer 3 (see RFC §2.3).
 *
 * Command handlers call `snapshotMatching` before every optimistic
 * patch. On error they call `restoreSnapshot`. On success they let
 * the natural refetch reconcile.
 *
 * These are the only rollback primitives; command handlers do NOT
 * manage rollback state by hand.
 */

import type { QueryClient, QueryKey } from '@tanstack/react-query'

export type Snapshot = ReadonlyArray<readonly [QueryKey, unknown]>

/**
 * Patch every cache entry matching `keyPrefix` with the same pure
 * updater. Guarantees that when a mutation applies to N cache lines
 * (e.g. the same thread appears in "Inbox" and "Starred" lists) all N
 * get patched consistently.
 */
export function patchMatching<T>(
  qc: QueryClient,
  keyPrefix: QueryKey,
  updater: (data: T) => T
): void {
  const entries = qc.getQueriesData<T>({ queryKey: keyPrefix })
  for (const [key, data] of entries) {
    if (data === undefined) continue
    qc.setQueryData<T>(key, updater(data))
  }
}

/**
 * Reverse of `snapshotMatching`. Restores every captured entry with
 * its exact pre-mutation value.
 */
export function restoreSnapshot(qc: QueryClient, snapshot: Snapshot): void {
  for (const [key, data] of snapshot) {
    qc.setQueryData(key, data)
  }
}

/**
 * Capture every current cache entry matching `keyPrefix` before an
 * optimistic mutation. Returned Snapshot is opaque to callers.
 */
export function snapshotMatching(qc: QueryClient, keyPrefix: QueryKey): Snapshot {
  const entries = qc.getQueriesData({ queryKey: keyPrefix })
  return entries.map(([key, data]) => [key, data] as const)
}
