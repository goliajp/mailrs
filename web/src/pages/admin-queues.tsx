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
import { adminKeys } from '@/lib/query-keys'
import { adminListGet, adminPost } from '@/wire/endpoints/admin'

const PAGE_SIZE = 20

// the states `GET /api/admin/queues` lists; finished jobs are not in it
const ALL_STATUSES = ['pending', 'retrying', 'inflight'] as const
/** One row of `GET /api/admin/queues`: the job's stored envelope, minus the message body. */
type QueueRow = {
  attempts?: number
  id: number
  last_error?: null | string
  recipient: string
  sender: string
  status: QueueStatus
}

type QueueStatus = (typeof ALL_STATUSES)[number]

const statusStyles: Record<QueueStatus, string> = {
  inflight: 'bg-info/10 text-info',
  pending: 'bg-accent/10 text-accent',
  retrying: 'bg-warning/10 text-warning',
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
    queryFn: ({ signal }) => adminListGet<QueueRow>('/admin/queues', signal),
  })
  const queue = useMemo(() => data ?? [], [data])

  const retryQueueItem = useAdminMutation({
    invalidateKey: adminKeys.queues(),
    mutationFn: (id: number) => adminPost(`/queue/${id}/retry`, {}),
    successMsg: (id) => `Queue item #${id} released for delivery`,
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

  const filtered = useMemo(() => {
    if (!statusFilter) return queue
    return queue.filter((q) => q.status === statusFilter)
  }, [queue, statusFilter])

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
          className={`${filterBaseStyle} ${filterAllStyle} ${activeIf(statusFilter === null)}`}
          onClick={() => setStatusFilter(null)}
        >
          All ({queue.length})
        </button>
        {ALL_STATUSES.map((status) => (
          <button
            aria-pressed={statusFilter === status}
            className={`${filterBaseStyle} ${statusStyles[status]} ${activeIf(statusFilter === status)}`}
            key={status}
            onClick={() => setStatusFilter(toggled(statusFilter, status))}
          >
            <span className="capitalize">{status}</span> ({counts[status]})
          </button>
        ))}
      </div>

      <QueueBody
        error={error}
        filtered={filtered}
        isFetching={isFetching}
        isPending={isPending}
        onPage={setCurrentPage}
        onRetry={() => refetch()}
        onRetryItem={(id) => retryQueueItem.mutate(id)}
        pageItems={pageItems}
        queue={queue}
        retrying={retryQueueItem.isPending}
        safePage={safePage}
        statusFilter={statusFilter}
        totalPages={totalPages}
      />
    </AdminPageShell>
  )
}

function activeIf(active: boolean): string {
  if (active) return filterActiveStyle
  return ''
}

function domainOf(address: string): string {
  const at = address.lastIndexOf('@')
  if (at < 0) return '—'
  return address.slice(at + 1)
}

function parseStatus(raw: null | string): null | QueueStatus {
  if (!raw) return null
  if ((ALL_STATUSES as readonly string[]).includes(raw)) return raw as QueueStatus
  return null
}

function QueueBody(p: {
  error: Error | null
  filtered: QueueRow[]
  isFetching: boolean
  isPending: boolean
  onPage: (page: number) => void
  onRetry: () => void
  onRetryItem: (id: number) => void
  pageItems: QueueRow[]
  queue: QueueRow[]
  retrying: boolean
  safePage: number
  statusFilter: null | QueueStatus
  totalPages: number
}) {
  if (p.isPending) return <AdminTableSkeleton cols={7} headers={HEADERS} rows={6} />
  if (p.error) {
    return <AdminErrorState error={p.error} onRetry={p.onRetry} retryDisabled={p.isFetching} />
  }
  if (p.queue.length === 0) {
    return (
      <AdminEmptyState
        description="Outbound mail will appear here while it's being delivered"
        icon={<Inbox className="h-10 w-10" />}
        title="Queue is empty"
      />
    )
  }
  return (
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
            {p.pageItems.map((item) => (
              <tr className="border-border border-b last:border-0" key={item.id}>
                <td className="px-4 py-3 font-medium">{item.sender}</td>
                <td className="px-4 py-3">{item.recipient}</td>
                <td className="text-fg-secondary px-4 py-3">{domainOf(item.recipient)}</td>
                <td className="px-4 py-3">
                  <span className={`rounded px-1.5 py-0.5 text-xs ${statusStyles[item.status]}`}>
                    {item.status}
                  </span>
                </td>
                <td className="px-4 py-3 tabular-nums">{item.attempts ?? 0}</td>
                <td className="text-fg-muted max-w-48 truncate px-4 py-3 text-xs">
                  {item.last_error || '—'}
                </td>
                <td className="px-4 py-3 text-right">
                  {item.status === 'retrying' && (
                    <button
                      className="text-accent text-xs transition-colors hover:opacity-70 disabled:opacity-50"
                      disabled={p.retrying}
                      onClick={() => p.onRetryItem(item.id)}
                      title="Send now instead of waiting for the next attempt"
                    >
                      Retry now
                    </button>
                  )}
                </td>
              </tr>
            ))}
            {p.pageItems.length === 0 && (
              <tr>
                <td className="text-fg-muted px-4 py-8 text-center" colSpan={7}>
                  No {p.statusFilter} entries
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </ScrollableTable>

      {p.filtered.length > PAGE_SIZE && (
        <div className="mt-4 flex items-center justify-between text-sm">
          <span className="text-fg-secondary">
            {p.filtered.length} entries &middot; Page {p.safePage} / {p.totalPages}
          </span>
          <div className="flex gap-2">
            <button
              className="border-border hover:bg-bg-secondary rounded-md border px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
              disabled={p.safePage <= 1}
              onClick={() => p.onPage(p.safePage - 1)}
            >
              Previous
            </button>
            <button
              className="border-border hover:bg-bg-secondary rounded-md border px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
              disabled={p.safePage >= p.totalPages}
              onClick={() => p.onPage(p.safePage + 1)}
            >
              Next
            </button>
          </div>
        </div>
      )}
    </>
  )
}

function toggled(current: null | QueueStatus, status: QueueStatus): null | QueueStatus {
  if (current === status) return null
  return status
}
