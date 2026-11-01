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
import { PausedSenders } from '@/components/admin-queues/paused-senders'
import { QueuePager, QueueStatusFilter } from '@/components/admin-queues/queue-controls'
import { ALL_STATUSES, statusLabels, statusStyles } from '@/components/admin-queues/queue-row'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { formatDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet, adminPost } from '@/wire/endpoints/admin'

const PAGE_SIZE = 20

const HEADERS: Key[] = [
  'admin.queue.from',
  'admin.queue.to',
  'admin.col.domain',
  'admin.col.status',
  'admin.queue.since',
  'admin.queue.attempts',
  'admin.queue.lastError',
]

export function AdminQueues() {
  const t = useT()
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
    successMsg: (id) => t('admin.queue.released', { id }),
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
      header: t('admin.queue.from'),
      key: 'from',
      cell: (q) => <span className="font-medium">{q.sender || '—'}</span>,
    },
    { header: t('admin.queue.to'), key: 'to', cell: (q) => q.recipient || '—' },
    {
      header: t('admin.col.domain'),
      key: 'domain',
      cell: (q) => <span className="text-fg-secondary">{domainOf(q.recipient)}</span>,
    },
    {
      header: t('admin.col.status'),
      key: 'status',
      cell: (q) => (
        <span className={`rounded px-1.5 py-0.5 text-xs ${statusStyles[q.status]}`}>
          {t(statusLabels[q.status])}
        </span>
      ),
    },
    {
      header: t('admin.queue.since'),
      key: 'since',
      cell: (q) => <span className="text-fg-secondary text-xs tabular-nums">{since(q)}</span>,
    },
    { align: 'end', header: t('admin.queue.attempts'), key: 'attempts', cell: (q) => attempts(q) },
    {
      header: t('admin.queue.lastError'),
      key: 'error',
      cell: (q) => (
        <span className="text-fg-muted block max-w-48 truncate text-xs" title={q.last_error ?? ''}>
          {q.last_error || '—'}
        </span>
      ),
    },
    {
      align: 'end',
      header: t('admin.col.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (q) => <RetryButton onRetry={() => setRetryTarget(q)} row={q} />,
    },
  ]

  const renderBody = () => {
    if (isPending) {
      return <AdminTableSkeleton cols={8} headers={[...HEADERS.map((h) => t(h)), '']} rows={6} />
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          retryDisabled={isFetching}
          what={t('admin.queue.listName')}
        />
      )
    }
    if (queue.length === 0) {
      return (
        <AdminEmptyState
          description={t('admin.queue.emptyHelp')}
          icon={<Inbox className="h-10 w-10" />}
          title={t('admin.queue.empty')}
        />
      )
    }
    if (pageItems.length === 0) {
      return (
        <p className="text-fg-muted py-8 text-center text-sm">
          {t('admin.queue.noneOf', { status: statusName(statusFilter, t) })}
        </p>
      )
    }
    return (
      <>
        <AdminTable
          columns={columns}
          label={t('admin.queue.title')}
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
          {t('admin.refresh')}
        </Button>
      }
      subtitle={t('admin.queue.subtitle')}
      title={t('admin.queue.title')}
    >
      <PausedSenders />

      <QueueStatusFilter onChange={setStatusFilter} queue={queue} value={statusFilter} />

      {renderBody()}

      {retryTarget && (
        <ConfirmAction
          confirmLabel={t(
            pendingLabel(retryQueueItem.isPending, 'admin.queue.retrying', 'admin.queue.retryNow')
          )}
          consequence={t('admin.queue.retryConsequence', {
            recipient: retryTarget.recipient,
            sender: retryTarget.sender,
          })}
          onCancel={() => setRetryTarget(null)}
          onConfirm={() =>
            retryQueueItem.mutate(retryTarget.id, { onSettled: () => setRetryTarget(null) })
          }
          pending={retryQueueItem.isPending}
          title={t('admin.queue.retryTitle', { id: retryTarget.id })}
        />
      )}
    </AdminPageShell>
  )
}

// a failed job keeps no attempt count, only why it stopped
function attempts(q: QueueRow): number | string {
  if (q.status === 'failed') return '—'
  return q.attempts ?? 0
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

function pendingLabel(pending: boolean, busy: Key, idle: Key): Key {
  if (pending) return busy
  return idle
}

// only a job waiting between attempts can be released; the backend answers 409 for the rest
function RetryButton({ onRetry, row }: { onRetry: () => void; row: QueueRow }) {
  const t = useT()
  if (row.status !== 'retrying') return null
  return (
    <button
      className="text-accent text-xs transition-colors hover:opacity-70"
      onClick={onRetry}
      title={t('admin.queue.retryHint')}
      type="button"
    >
      {t('admin.queue.retryNow')}
    </button>
  )
}

// when the job failed for good, or when it was queued
function since(q: QueueRow): string {
  let at = q.created_at
  if (q.status === 'failed') at = q.failed_at
  if (!at) return '—'
  return formatDate(at)
}

function statusName(status: null | QueueStatus, t: ReturnType<typeof useT>): string {
  if (status === null) return ''
  return t(statusLabels[status])
}
