import type { GreylistForm } from './admin-greylist-form'
import type { Column } from '@/components/admin-page'
import type { GreylistLocalEntry, GreylistLocalKind, GreylistLocalListName } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Shield } from 'lucide-react'
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
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost } from '@/wire/endpoints/admin'

import { GreylistAddForm } from './admin-greylist-form'

/**
 * One row of `GET /api/admin/greylist/local-lists`. `id` is the stored
 * key: a number for entries made here or through MCP, a string such as
 * `not-junk:a@b.c` for the ones marking a conversation not junk wrote
 * before it used the same format.
 */
type GreylistRow = Omit<GreylistLocalEntry, 'id'> & { id: number | string }

const HEADERS = ['Value', 'Kind', 'List', 'Note', 'Added', 'By', '']
const NO_ROWS: GreylistRow[] = []
const KINDS: GreylistLocalKind[] = ['domain', 'email', 'cidr']
const LISTS: GreylistLocalListName[] = ['white', 'black']
const KIND_LABELS: Record<GreylistLocalKind, string> = {
  cidr: 'CIDR',
  domain: 'Domain',
  email: 'Email',
}
const LIST_LABELS: Record<GreylistLocalListName, string> = { black: 'Black', white: 'White' }

// the receiver builds the greylist stage, and with it these lists, only when this is set
const ENABLE_NOTE =
  'The receiver reads these lists only while greylisting is on (MAILRS_GREYLIST_ENABLED=1); with it off, the default, no entry has any effect'

export function AdminGreylist() {
  const [searchParams, setSearchParams] = useSearchParams()
  const filterKind = pick(searchParams.get('kind'), KINDS)
  const filterList = pick(searchParams.get('list'), LISTS)
  const [adding, setAdding] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<GreylistRow | null>(null)

  const setFilter = (key: 'kind' | 'list', value: string) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (value === 'all') next.delete(key)
      else next.set(key, value)
      return next
    })
  }

  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.greylistLocal(),
    queryFn: ({ signal }) => adminListGet<GreylistRow>('/admin/greylist/local-lists', signal),
  })
  const entries = data ?? NO_ROWS

  const filtered = useMemo(
    () =>
      entries.filter(
        (e) => (!filterKind || e.kind === filterKind) && (!filterList || e.list === filterList)
      ),
    [entries, filterKind, filterList]
  )

  const addEntry = useAdminMutation({
    invalidateKey: adminKeys.greylistLocal(),
    mutationFn: (f: GreylistForm) =>
      adminPost('/admin/greylist/local-lists', {
        kind: f.kind,
        list: f.list,
        note: f.note.trim() || null,
        value: f.value.trim(),
      }),
    successMsg: (f) => `"${f.value.trim()}" added to the ${f.list} list`,
  })

  const deleteEntry = useAdminMutation({
    invalidateKey: adminKeys.greylistLocal(),
    successMsg: 'Entry deleted',
    mutationFn: (id: number | string) =>
      adminDelete(`/admin/greylist/local-lists/${encodeURIComponent(String(id))}`),
  })

  const columns: Column<GreylistRow>[] = [
    {
      header: 'Value',
      key: 'value',
      cell: (e) => <span className="font-mono text-xs">{e.value}</span>,
    },
    {
      header: 'Kind',
      key: 'kind',
      cell: (e) => <span className="text-fg-secondary">{KIND_LABELS[e.kind]}</span>,
    },
    {
      header: 'List',
      key: 'list',
      cell: (e) => (
        <span className={`rounded px-2 py-0.5 text-xs font-medium ${listStyle(e.list)}`}>
          {LIST_LABELS[e.list]}
        </span>
      ),
    },
    {
      header: 'Note',
      key: 'note',
      cell: (e) => <span className="text-fg-secondary text-xs">{e.note ?? '—'}</span>,
    },
    {
      align: 'end',
      header: 'Added',
      key: 'created',
      cell: (e) => <span className="text-fg-muted text-xs">{formatCreated(e.created_at)}</span>,
    },
    {
      header: 'By',
      key: 'by',
      cell: (e) => <span className="text-fg-muted text-xs">{e.created_by ?? '—'}</span>,
    },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (e) => (
        <button
          className="text-danger text-xs transition-colors hover:opacity-70"
          onClick={() => setDeleteTarget(e)}
          type="button"
        >
          Delete
        </button>
      ),
    },
  ]

  const renderBody = () => {
    if (isPending) return <AdminTableSkeleton cols={HEADERS.length} headers={HEADERS} rows={5} />
    if (error) {
      return <AdminErrorState error={error} onRetry={() => refetch()} what="the greylist entries" />
    }
    if (entries.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description={`Add a sender's domain, address or network here to let it skip greylisting (white) or to refuse its mail (black). ${ENABLE_NOTE}`}
          icon={<Shield className="h-10 w-10" />}
          title="No greylist entries yet"
        />
      )
    }
    if (filtered.length === 0) {
      return (
        <p className="text-fg-muted py-8 text-center text-sm">No entries match these filters</p>
      )
    }
    return (
      <AdminTable
        columns={columns}
        label="Greylist entries"
        rowKey={(e) => String(e.id)}
        rows={filtered}
      />
    )
  }

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
            type="button"
          >
            Add entry
          </button>
        )
      }
      subtitle={`White entries skip greylisting, black entries are refused with SMTP 550 5.7.1, and black wins when a sender matches both. ${ENABLE_NOTE}`}
      title="Greylist"
    >
      <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
        <select
          aria-label="Filter by kind"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setFilter('kind', e.target.value)}
          value={filterKind ?? 'all'}
        >
          <option value="all">All kinds</option>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {KIND_LABELS[k]}
            </option>
          ))}
        </select>
        <select
          aria-label="Filter by list"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setFilter('list', e.target.value)}
          value={filterList ?? 'all'}
        >
          <option value="all">Both lists</option>
          {LISTS.map((l) => (
            <option key={l} value={l}>
              {LIST_LABELS[l]}
            </option>
          ))}
        </select>
      </div>

      {adding && (
        <GreylistAddForm
          onCancel={() => setAdding(false)}
          onSave={(form, done) =>
            addEntry.mutate(form, {
              onSuccess: () => {
                done()
                setAdding(false)
              },
            })
          }
          saving={addEntry.isPending}
        />
      )}

      {renderBody()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={busyLabel(deleteEntry.isPending, 'Delete entry', 'Deleting...')}
          consequence={deleteConsequence(deleteTarget)}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() =>
            deleteEntry.mutate(deleteTarget.id, { onSettled: () => setDeleteTarget(null) })
          }
          pending={deleteEntry.isPending}
          title={`Delete ${deleteTarget.value} from the ${deleteTarget.list} list?`}
        />
      )}
    </AdminPageShell>
  )
}

function busyLabel(busy: boolean, idle: string, working: string): string {
  if (busy) return working
  return idle
}

function deleteConsequence(entry: GreylistRow): string {
  const reload =
    'The entry is deleted, and the receiver drops it at its next reload, within a minute. This only matters while greylisting is on (MAILRS_GREYLIST_ENABLED=1)'
  if (entry.list === 'black') {
    return `Mail from ${entry.value} is no longer refused by this entry. ${reload}`
  }
  return `Mail from ${entry.value} no longer skips greylisting through this entry and is greylisted like anyone else's. ${reload}`
}

function formatCreated(unix: number): string {
  if (!unix) return '—'
  const iso = new Date(unix * 1000).toISOString()
  return `${iso.slice(0, 10)} ${iso.slice(11, 16)}`
}

function listStyle(list: GreylistLocalListName): string {
  if (list === 'black') return 'bg-danger/10 text-danger'
  return 'bg-accent/10 text-accent'
}

function pick<T extends string>(raw: null | string, allowed: readonly T[]): null | T {
  if (raw === null) return null
  return allowed.find((a) => a === raw) ?? null
}
