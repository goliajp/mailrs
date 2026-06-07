import type { QueueEntry } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { useCallback, useMemo, useState } from 'react'

import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { fetchJson, postJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

const PAGE_SIZE = 20

const ALL_STATUSES = ['pending', 'inflight', 'delivered', 'failed', 'bounced'] as const
type QueueStatus = (typeof ALL_STATUSES)[number]

const statusStyles: Record<string, string> = {
  bounced: 'bg-warning/10 text-warning',
  delivered: 'bg-success/10 text-success',
  failed: 'bg-danger/10 text-danger',
  inflight: 'bg-info/10 text-info',
  pending: 'bg-accent/10 text-accent',
}

const filterBaseStyle = 'rounded-md px-3 py-1 text-xs font-medium transition-colors cursor-pointer'
const filterActiveStyle = 'ring-2 ring-offset-1 ring-border ring-offset-bg'
const filterAllStyle = 'bg-border text-fg-secondary'

export function AdminQueues() {
  const { data: queue = [], refetch } = useQuery({
    queryKey: adminKeys.queues(),
    refetchInterval: 5000,
    queryFn: ({ signal }) => fetchJson<QueueEntry[]>('/queue', signal),
  })
  const [statusFilter, setStatusFilterRaw] = useState<null | QueueStatus>(null)
  const [currentPage, setCurrentPage] = useState(1)

  // wrap setStatusFilter to also reset page
  const setStatusFilter = useCallback((v: null | QueueStatus) => {
    setStatusFilterRaw(v)
    setCurrentPage(1)
  }, [])

  const retryQueueItem = useAdminMutation({
    invalidateKey: adminKeys.queues(),
    mutationFn: (id: number) => postJson(`/queue/${id}/retry`, {}),
    successMsg: (id) => `Retrying queue item #${id}`,
  })

  const handleRetry = (id: number) => {
    retryQueueItem.mutate(id)
  }

  const counts = useMemo(() => {
    const result: Record<string, number> = {}
    for (const s of ALL_STATUSES) {
      result[s] = 0
    }
    for (const entry of queue) {
      if (entry.status in result) {
        result[entry.status] += 1
      }
    }
    return result
  }, [queue])

  const filtered = useMemo(
    () => (statusFilter ? queue.filter((q) => q.status === statusFilter) : queue),
    [queue, statusFilter]
  )

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))

  // clamp current page if data shrinks
  const safePage = Math.min(currentPage, totalPages)
  if (safePage !== currentPage) {
    setCurrentPage(safePage)
  }

  const pageItems = useMemo(
    () => filtered.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE),
    [filtered, safePage]
  )

  const handleFilterClick = (status: QueueStatus) => {
    setStatusFilter(statusFilter === status ? null : status)
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Outbound Queue</h2>
        <button
          className="bg-surface hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
          onClick={() => refetch()}
        >
          Refresh
        </button>
      </div>

      {/* status counts as clickable filter chips */}
      <div className="mb-4 flex flex-wrap gap-2">
        <button
          className={`${filterBaseStyle} ${filterAllStyle} ${statusFilter === null ? filterActiveStyle : ''}`}
          onClick={() => setStatusFilter(null)}
        >
          All ({queue.length})
        </button>
        {ALL_STATUSES.map((status) => (
          <button
            className={`${filterBaseStyle} ${statusStyles[status]} ${statusFilter === status ? filterActiveStyle : ''}`}
            key={status}
            onClick={() => handleFilterClick(status)}
          >
            <span className="capitalize">{status}</span> ({counts[status]})
          </button>
        ))}
      </div>

      {/* table */}
      <div className="border-border overflow-hidden rounded-lg border">
        <ScrollableTable>
          <table className="w-full text-left text-sm">
            <thead className="border-border bg-bg-secondary border-b">
              <tr>
                <th className="px-4 py-2.5 font-medium">From</th>
                <th className="px-4 py-2.5 font-medium">To</th>
                <th className="px-4 py-2.5 font-medium">Domain</th>
                <th className="px-4 py-2.5 font-medium">Status</th>
                <th className="px-4 py-2.5 font-medium">Attempts</th>
                <th className="px-4 py-2.5 font-medium">Error</th>
                <th className="px-4 py-2.5 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {pageItems.map((item) => (
                <tr className="border-border border-b last:border-0" key={item.id}>
                  <td className="px-4 py-3 font-medium">{item.sender}</td>
                  <td className="px-4 py-3">{item.recipient}</td>
                  <td className="text-fg-secondary px-4 py-3">{item.domain}</td>
                  <td className="px-4 py-3">
                    <span
                      className={`rounded px-1.5 py-0.5 text-xs ${statusStyles[item.status] ?? ''}`}
                    >
                      {item.status}
                    </span>
                  </td>
                  <td className="px-4 py-3 tabular-nums">{item.attempts}</td>
                  <td className="text-fg-muted max-w-48 truncate px-4 py-3 text-xs">
                    {item.last_error ?? '—'}
                  </td>
                  <td className="px-4 py-3 text-right">
                    {item.status === 'failed' && (
                      <button
                        className="text-accent text-xs transition-colors hover:opacity-70"
                        onClick={() => handleRetry(item.id)}
                      >
                        Retry
                      </button>
                    )}
                  </td>
                </tr>
              ))}
              {pageItems.length === 0 && (
                <tr>
                  <td className="text-fg-muted px-4 py-8 text-center" colSpan={7}>
                    {statusFilter ? `No ${statusFilter} entries` : 'Queue is empty'}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </ScrollableTable>
      </div>

      {/* pagination */}
      {filtered.length > PAGE_SIZE && (
        <div className="mt-4 flex items-center justify-between text-sm">
          <span className="text-fg-secondary">
            {filtered.length} entries &middot; Page {safePage} / {totalPages}
          </span>
          <div className="flex gap-2">
            <button
              className="border-border hover:bg-bg-secondary rounded-md border px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
              disabled={safePage <= 1}
              onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
            >
              Previous
            </button>
            <button
              className="border-border hover:bg-bg-secondary rounded-md border px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
              disabled={safePage >= totalPages}
              onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
            >
              Next
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
