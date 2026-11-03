import { Button } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { ScrollText } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'

import { DeletedNoteButton } from '@/components/admin-deleted-note'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
} from '@/components/admin-page'
import { type Key, useT } from '@/i18n'
import { formatDate, formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet } from '@/wire/endpoints/admin'

import { ACTION_GROUPS, actionColor, matchesActionFilter } from './admin-audit-actions'

type AuditEntry = {
  action: string
  actor: string
  detail: string
  id: number
  target: string
  timestamp: number
}

// the page asks for this many and filters them in the browser
const LIMIT = 200
const HEADERS: Key[] = [
  'admin.audit.action',
  'admin.audit.actor',
  'admin.aliases.target',
  'admin.audit.detail',
  'admin.time',
]
const NO_ENTRIES: AuditEntry[] = []

export function AdminAuditLog() {
  const t = useT()
  const [params, setParams] = useSearchParams()
  const actionFilter = params.get('action') ?? ''
  const actorFilter = params.get('actor') ?? ''
  const [open, setOpen] = useState<ReadonlySet<number>>(new Set())

  const setParam = (name: string, value: string) => {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev)
        if (value) next.set(name, value)
        else next.delete(name)
        return next
      },
      { replace: true }
    )
  }

  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.auditLog(),
    queryFn: ({ signal }) => adminListGet<AuditEntry>(`/admin/audit-log?limit=${LIMIT}`, signal),
  })
  const entries = data ?? NO_ENTRIES

  const filtered = useMemo(
    () =>
      entries.filter(
        (e) => matchesActionFilter(e.action, actionFilter) && actorMatches(e.actor, actorFilter)
      ),
    [entries, actionFilter, actorFilter]
  )

  const toggle = (id: number) => {
    setOpen((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const columns: Column<AuditEntry>[] = [
    {
      header: t('admin.audit.action'),
      key: 'action',
      cell: (e) => <span className={`font-mono text-xs ${actionColor(e.action)}`}>{e.action}</span>,
    },
    {
      header: t('admin.audit.actor'),
      key: 'actor',
      cell: (e) => <span className="font-medium">{e.actor}</span>,
    },
    {
      header: t('admin.aliases.target'),
      key: 'target',
      cell: (e) => <span className="text-fg-secondary break-all">{e.target}</span>,
    },
    {
      header: t('admin.audit.detail'),
      key: 'detail',
      cell: (e) => (
        <span className="text-fg-muted block max-w-xs truncate" title={e.detail}>
          {e.detail}
        </span>
      ),
    },
    {
      align: 'end',
      header: t('admin.time'),
      key: 'time',
      cell: (e) => (
        <span className="text-fg-secondary whitespace-nowrap" title={formatFullDate(e.timestamp)}>
          {formatDate(e.timestamp)}
        </span>
      ),
    },
    {
      align: 'end',
      header: t('admin.col.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (e) => (
        <span className="flex justify-end gap-3">
          <DeletedNoteButton action={e.action} detail={e.detail} target={e.target} />
          <DetailToggle entry={e} onToggle={toggle} open={open.has(e.id)} />
        </span>
      ),
    },
  ]

  const detailOf = (e: AuditEntry) => {
    if (!open.has(e.id)) return null
    return (
      <pre className="bg-bg-secondary text-fg max-w-full overflow-auto rounded-md p-3 font-mono text-xs">
        {prettyJson(e.detail) ?? e.detail}
      </pre>
    )
  }

  const renderBody = () => {
    if (isPending) {
      return <AdminTableSkeleton cols={6} headers={[...HEADERS.map((h) => t(h)), '']} rows={6} />
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          retryDisabled={isFetching}
          what={t('admin.audit.listName')}
        />
      )
    }
    if (entries.length === 0) {
      return (
        <AdminEmptyState
          description={t('admin.audit.emptyHelp')}
          icon={<ScrollText className="h-10 w-10" />}
          title={t('admin.audit.empty')}
        />
      )
    }
    if (filtered.length === 0) {
      return (
        <AdminEmptyState
          description={t('admin.audit.noMatchHelp', { n: entries.length })}
          icon={<ScrollText className="h-10 w-10" />}
          title={t('admin.audit.noMatch')}
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={detailOf}
        label={t('admin.auditLog')}
        rowKey={(e) => String(e.id)}
        rows={filtered}
      />
    )
  }

  return (
    <AdminPageShell
      actions={
        <Button
          disabled={isFetching}
          loading={isFetching}
          onClick={() => refetch()}
          size="sm"
          variant="secondary"
        >
          {t('admin.refresh')}
        </Button>
      }
      subtitle={t('admin.audit.subtitle', { n: LIMIT })}
      title={t('admin.auditLog')}
    >
      <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
        <select
          aria-label={t('admin.audit.filterAction')}
          className="border-border bg-bg-secondary rounded-md border px-2 py-1.5 text-sm"
          onChange={(e) => setParam('action', e.target.value)}
          value={actionFilter}
        >
          {ACTION_GROUPS.map((g) => (
            <option key={g.value} value={g.value}>
              {t(g.label)}
            </option>
          ))}
        </select>
        <input
          aria-label={t('admin.audit.filterActor')}
          className="border-border bg-bg-secondary w-64 max-w-full rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setParam('actor', e.target.value)}
          placeholder={t('admin.audit.actorPlaceholder')}
          value={actorFilter}
        />
        <span className="text-fg-muted ml-auto text-xs tabular-nums">
          {t('admin.nOfTotal', { n: filtered.length, total: entries.length })}
        </span>
      </div>
      {renderBody()}
    </AdminPageShell>
  )
}

function actorMatches(actor: string, filter: string): boolean {
  if (filter === '') return true
  return actor.toLowerCase().includes(filter.toLowerCase())
}

/** A detail too long for its cell can be opened under the row. */
function DetailToggle({
  entry,
  onToggle,
  open,
}: {
  entry: AuditEntry
  onToggle: (id: number) => void
  open: boolean
}) {
  const t = useT()
  if (entry.detail.length <= 40 && prettyJson(entry.detail) === null) return null
  return (
    <button
      className="text-accent text-xs hover:opacity-80"
      onClick={() => onToggle(entry.id)}
      type="button"
    >
      {t(toggleLabel(open))}
    </button>
  )
}

function prettyJson(detail: string): null | string {
  const trimmed = detail.trim()
  if (!trimmed.startsWith('{') || !trimmed.endsWith('}')) return null
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2)
  } catch {
    return null
  }
}

function toggleLabel(open: boolean): Key {
  if (open) return 'admin.audit.hideDetail'
  return 'admin.audit.showDetail'
}
