import type { Column } from '@/components/admin-page'
import type { QueueRow, QueueStatus } from '@/components/admin-queues/queue-row'

import { Button } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { Inbox } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  ConfirmAction,
} from '@/components/admin-page'
import { QueuePager, QueueStatusFilter } from '@/components/admin-queues/queue-controls'
import { ALL_STATUSES, statusStyles } from '@/components/admin-queues/queue-row'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet, adminPost } from '@/wire/endpoints/admin'

const PAGE_SIZE = 20

const HEADERS = ['From', 'To', 'Domain', 'Status', 'Attempts', 'Last error', '']

export function AdminQueues() {
  const [searchParams, setSearchParams] = useSearchParams()
  const statusFilter = parseStatus(searchParams.get('status'))
  const currentPage = Math.max(1, Number.parseInt(searchParams.get('page') ?? '1', 10) || 1)
  const [retryTarget, setRetryTarget] = useState<null | QueueRow>(null)

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
    successMsg: (id) => `Job #${id} released for delivery`,
  })

  const filtered = useMemo(() => {
    if (!statusFilter) return queue
    return queue.filter((q) => q.status === statusFilter)
  }, [queue, statusFilter])

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
  const safePage = Math.min(currentPage, totalPages)
  const pageItems = filtered.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE)

  const columns: Column<QueueRow>[] = [
    {
      header: 'From',
      key: 'from',
      cell: (q) => <span className="font-medium">{q.sender}</span>,
    },
    { header: 'To', key: 'to', cell: (q) => q.recipient },
    {
      header: 'Domain',
      key: 'domain',
      cell: (q) => <span className="text-fg-secondary">{domainOf(q.recipient)}</span>,
    },
    {
      header: 'Status',
      key: 'status',
      cell: (q) => (
        <span className={`rounded px-1.5 py-0.5 text-xs ${statusStyles[q.status]}`}>
          {q.status}
        </span>
      ),
    },
    { align: 'end', header: 'Attempts', key: 'attempts', cell: (q) => q.attempts ?? 0 },
    {
      header: 'Last error',
      key: 'error',
      cell: (q) => (
        <span className="text-fg-muted block max-w-48 truncate text-xs" title={q.last_error ?? ''}>
          {q.last_error || '—'}
        </span>
      ),
    },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (q) => <RetryButton onRetry={() => setRetryTarget(q)} row={q} />,
    },
  ]

  const renderBody = () => {
    if (isPending) return <AdminTableSkeleton cols={7} headers={HEADERS} rows={6} />
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          retryDisabled={isFetching}
          what="the outbound queue"
        />
      )
    }
    if (queue.length === 0) {
      return (
        <AdminEmptyState
          description="Nothing is waiting to be sent. Outbound mail shows here while it waits for delivery or between attempts"
          icon={<Inbox className="h-10 w-10" />}
          title="The queue is empty"
        />
      )
    }
    if (pageItems.length === 0) {
      return <p className="text-fg-muted py-8 text-center text-sm">No {statusFilter} jobs</p>
    }
    return (
      <>
        <AdminTable
          columns={columns}
          label="Outbound queue"
          rowKey={(q) => String(q.id)}
          rows={pageItems}
        />
        <QueuePager
          onPage={setCurrentPage}
          page={safePage}
          pageSize={PAGE_SIZE}
          total={filtered.length}
          totalPages={totalPages}
        />
      </>
    )
  }

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
      subtitle="Mail waiting to leave this server, up to 100 jobs per state. Delivered mail leaves the list, and so does mail that failed for good"
      title="Outbound queue"
    >
      <QueueStatusFilter onChange={setStatusFilter} queue={queue} value={statusFilter} />

      {renderBody()}

      {retryTarget && (
        <ConfirmAction
          confirmLabel={pendingLabel(retryQueueItem.isPending, 'Retrying...', 'Retry now')}
          consequence={retryConsequence(retryTarget)}
          onCancel={() => setRetryTarget(null)}
          onConfirm={() =>
            retryQueueItem.mutate(retryTarget.id, { onSettled: () => setRetryTarget(null) })
          }
          pending={retryQueueItem.isPending}
          title={`Retry job #${retryTarget.id} now?`}
        />
      )}
    </AdminPageShell>
  )
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

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}

// only a job waiting between attempts can be released; the backend answers 409 for the rest
function RetryButton({ onRetry, row }: { onRetry: () => void; row: QueueRow }) {
  if (row.status !== 'retrying') return null
  return (
    <button
      className="text-accent text-xs transition-colors hover:opacity-70"
      onClick={onRetry}
      title="Send now instead of waiting for the next attempt"
      type="button"
    >
      Retry now
    </button>
  )
}

function retryConsequence(row: QueueRow): string {
  return `The message to ${row.recipient} stops waiting out its backoff and joins the send queue now. It still counts as an attempt: a temporary failure parks it again with a longer wait, a permanent one moves it to failed and sends ${row.sender} a bounce notice`
}
