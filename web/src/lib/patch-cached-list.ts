import type { QueryClient, QueryKey } from '@tanstack/react-query'

/**
 * Write `data` into a cached list without making it look freshly fetched.
 *
 * `setQueryData` stamps the entry as fetched just now and clears any
 * invalidation on it. A patch is not a fetch: it edits the rows the cache
 * already has and cannot add the ones it never saw. Marking a thread read
 * patched every cached list this way, so a list invalidated because new
 * mail had arrived came back fresh, without the mail, and was shown as is
 * for the next thirty seconds (reported 2026-10-19: new mail showed under
 * Unread but not in N & P until a reload).
 *
 * So the entry keeps the time it was really fetched, and stays invalidated
 * if it was: the next reader refetches it as it would have.
 */
export function patchCachedList<T>(qc: QueryClient, key: QueryKey, data: T): void {
  const query = qc.getQueryCache().find({ exact: true, queryKey: key })
  const fetchedAt = query?.state.dataUpdatedAt
  const invalidated = query?.state.isInvalidated ?? false
  qc.setQueryData<T>(key, data, { updatedAt: fetchedAt })
  if (invalidated) {
    qc.invalidateQueries({ exact: true, queryKey: key, refetchType: 'none' }).catch(() => {})
  }
}
