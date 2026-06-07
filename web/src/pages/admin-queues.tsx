import type { QueueEntry } from '@/lib/types'

import { Button } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { Inbox } from 'lucide-react'
import { useMemo } from 'react'
import { useSearchParams } from 'react-router'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
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

const HEADERS = ['From', 'To', 'Domain', 'Status', 'Attempts', 'Error', 'Actions']

export function AdminQueues() {
  const [searchParams, setSearchParams] = useSearchParams()
  const statusFilter = parseStatus(searchParams.get('status'))
  const currentPage = Math.max(1, Number.parseInt(searchParams.get('page') ?? '1', 10) || 1)

  const setStatusFilter = (v: null | QueueStatus) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (v) next.set('status', v)
      else next.delete('status')
      next.delete('page')
      return next
    })
  }

  const setCurrentPage = (p: number) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (p === 1) next.delete('page')
      else next.set('page', String(p))
      return next
    })
  }

  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.queues(),
    refetchInterval: 5000,
    queryFn: ({ signal }) => fetchJson<QueueEntry[]>('/queue', signal),
  })
  const queue = useMemo(() => data ?? [], [data])

  const retryQueueItem = useAdminMutation({
    invalidateKey: adminKeys.queues(),
    mutationFn: (id: number) => postJson(`/queue/${id}/retry`, {}),
    successMsg: (id) => `Retrying queue item #${id}`,
  })

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
  const safePage = Math.min(currentPage, totalPages)

  const pageItems = useMemo(
    () => filtered.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE),
    [filtered, safePage]
  )

  return (
    <AdminPageShell
      actions={
        <Button
          disabled={isFetching && !isPending}
          loading={isFetching && !isPending}
          onClick={() => refetch()}
          size="sm"
          variant="secondary"
        >
          Refresh
        </Button>
      }
      title="Outbound Queue"
    >
      <div className="mb-4 flex flex-wrap gap-2">
        <button
          aria-pressed={statusFilter === null}
          className={`${filterBaseStyle} ${filterAllStyle} ${statusFilter === null ? filterActiveStyle : ''}`}
          onClick={() => setStatusFilter(null)}
        >
          All ({queue.length})
        </button>
        {ALL_STATUSES.map((status) => (
          <button
            aria-pressed={statusFilter === status}
            className={`${filterBaseStyle} ${statusStyles[status]} ${statusFilter === status ? filterActiveStyle : ''}`}
            key={status}
            onClick={() => setStatusFilter(statusFilter === status ? null : status)}
          >
            <span className="capitalize">{status}</span> ({counts[status]})
          </button>
        ))}
      </div>

      {isPending ? (
        <AdminTableSkeleton cols={7} headers={HEADERS} rows={6} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} retryDisabled={isFetching} />
      ) : queue.length === 0 ? (
        <AdminEmptyState
          description="Outbound mail will appear here while it's being delivered."
          icon={<Inbox className="h-10 w-10" />}
          title="Queue is empty"
        />
      ) : (
        <>
          <ScrollableTable>
            <table className="w-full text-left text-sm">
              <thead className="border-border bg-bg-secondary border-b">
                <tr>
                  {HEADERS.slice(0, -1).map((h) => (
                    <th className="px-4 py-2.5 font-medium" key={h}>
                      {h}
                    </th>
                  ))}
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
                          className="text-accent text-xs transition-colors hover:opacity-70 disabled:opacity-50"
                          disabled={retryQueueItem.isPending}
                          onClick={() => retryQueueItem.mutate(item.id)}
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
                      No {statusFilter} entries
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </ScrollableTable>

          {filtered.length > PAGE_SIZE && (
            <div className="mt-4 flex items-center justify-between text-sm">
              <span className="text-fg-secondary">
                {filtered.length} entries &middot; Page {safePage} / {totalPages}
              </span>
              <div className="flex gap-2">
                <button
                  className="border-border hover:bg-bg-secondary rounded-md border px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
                  disabled={safePage <= 1}
                  onClick={() => setCurrentPage(safePage - 1)}
                >
                  Previous
                </button>
                <button
                  className="border-border hover:bg-bg-secondary rounded-md border px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
                  disabled={safePage >= totalPages}
                  onClick={() => setCurrentPage(safePage + 1)}
                >
                  Next
                </button>
              </div>
            </div>
          )}
        </>
      )}
    </AdminPageShell>
  )
}

function parseStatus(raw: null | string): null | QueueStatus {
  if (!raw) return null
  return (ALL_STATUSES as readonly string[]).includes(raw) ? (raw as QueueStatus) : null
}
