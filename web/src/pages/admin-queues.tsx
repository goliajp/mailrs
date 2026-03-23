import { useAtom } from 'jotai'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { fetchJson, postJson } from '@/lib/api'
import type { QueueEntry } from '@/lib/types'
import { queueAtom } from '@/store/admin'

const PAGE_SIZE = 20

const ALL_STATUSES = ['pending', 'inflight', 'delivered', 'failed', 'bounced'] as const
type QueueStatus = (typeof ALL_STATUSES)[number]

const statusStyles: Record<string, string> = {
  pending: 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]',
  inflight: 'bg-[var(--color-status-info-subtle)] text-[var(--color-status-info)]',
  delivered: 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]',
  failed: 'bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]',
  bounced: 'bg-[var(--color-status-warning-subtle)] text-[var(--color-status-warning)]',
}

const filterBaseStyle = 'rounded-md px-3 py-1 text-xs font-medium transition-colors cursor-pointer'
const filterActiveStyle =
  'ring-2 ring-offset-1 ring-[var(--color-border-default)] ring-offset-[var(--color-bg-base)]'
const filterAllStyle = 'bg-[var(--color-border-default)] text-[var(--color-text-secondary)]'

export function AdminQueues() {
  const [queue, setQueue] = useAtom(queueAtom)
  const [statusFilter, setStatusFilterRaw] = useState<QueueStatus | null>(null)
  const [currentPage, setCurrentPage] = useState(1)

  // wrap setStatusFilter to also reset page
  const setStatusFilter = useCallback((v: QueueStatus | null) => {
    setStatusFilterRaw(v)
    setCurrentPage(1)
  }, [])

  const loadQueue = useCallback(async () => {
    try {
      const data = await fetchJson<QueueEntry[]>('/queue')
      setQueue(data)
    } catch {
      // keep current state on error
    }
  }, [setQueue])

  useEffect(() => {
    loadQueue()
    const interval = setInterval(loadQueue, 5000)
    return () => clearInterval(interval)
  }, [loadQueue])

  const handleRetry = async (id: number) => {
    await postJson(`/queue/${id}/retry`, {})
    loadQueue()
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
    [queue, statusFilter],
  )

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))

  // clamp current page if data shrinks
  const safePage = Math.min(currentPage, totalPages)
  if (safePage !== currentPage) {
    setCurrentPage(safePage)
  }

  const pageItems = useMemo(
    () => filtered.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE),
    [filtered, safePage],
  )

  const handleFilterClick = (status: QueueStatus) => {
    setStatusFilter(statusFilter === status ? null : status)
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Outbound Queue</h2>
        <button
          onClick={loadQueue}
          className="rounded-md bg-[var(--color-bg-raised)] px-3 py-1.5 text-sm transition-colors hover:bg-[var(--color-hover)]"
        >
          Refresh
        </button>
      </div>

      {/* status counts as clickable filter chips */}
      <div className="mb-4 flex flex-wrap gap-2">
        <button
          onClick={() => setStatusFilter(null)}
          className={`${filterBaseStyle} ${filterAllStyle} ${statusFilter === null ? filterActiveStyle : ''}`}
        >
          All ({queue.length})
        </button>
        {ALL_STATUSES.map((status) => (
          <button
            key={status}
            onClick={() => handleFilterClick(status)}
            className={`${filterBaseStyle} ${statusStyles[status]} ${statusFilter === status ? filterActiveStyle : ''}`}
          >
            <span className="capitalize">{status}</span> ({counts[status]})
          </button>
        ))}
      </div>

      {/* table */}
      <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
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
              <tr
                key={item.id}
                className="border-b border-[var(--color-border-default)] last:border-0"
              >
                <td className="px-4 py-3 font-medium">{item.sender}</td>
                <td className="px-4 py-3">{item.recipient}</td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)]">{item.domain}</td>
                <td className="px-4 py-3">
                  <span
                    className={`rounded px-1.5 py-0.5 text-xs ${statusStyles[item.status] ?? ''}`}
                  >
                    {item.status}
                  </span>
                </td>
                <td className="px-4 py-3 tabular-nums">{item.attempts}</td>
                <td className="max-w-48 truncate px-4 py-3 text-xs text-[var(--color-text-tertiary)]">
                  {item.last_error ?? '—'}
                </td>
                <td className="px-4 py-3 text-right">
                  {item.status === 'failed' && (
                    <button
                      onClick={() => handleRetry(item.id)}
                      className="text-xs text-[var(--color-brand-primary)] transition-colors hover:opacity-70"
                    >
                      Retry
                    </button>
                  )}
                </td>
              </tr>
            ))}
            {pageItems.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-8 text-center text-[var(--color-text-tertiary)]">
                  {statusFilter ? `No ${statusFilter} entries` : 'Queue is empty'}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/* pagination */}
      {filtered.length > PAGE_SIZE && (
        <div className="mt-4 flex items-center justify-between text-sm">
          <span className="text-[var(--color-text-secondary)]">
            {filtered.length} entries &middot; Page {safePage} / {totalPages}
          </span>
          <div className="flex gap-2">
            <button
              disabled={safePage <= 1}
              onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
              className="rounded-md border border-[var(--color-border-default)] px-3 py-1.5 transition-colors hover:bg-[var(--color-hover)] disabled:cursor-not-allowed disabled:opacity-40"
            >
              Previous
            </button>
            <button
              disabled={safePage >= totalPages}
              onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
              className="rounded-md border border-[var(--color-border-default)] px-3 py-1.5 transition-colors hover:bg-[var(--color-hover)] disabled:cursor-not-allowed disabled:opacity-40"
            >
              Next
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
