import type { GreylistForm } from './admin-greylist-form'
import type { GreylistLocalEntry, GreylistLocalKind, GreylistLocalListName } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Shield } from 'lucide-react'
import { useMemo, useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { MobileModal } from '@/components/mobile-modal'
import { ScrollableTable } from '@/components/scrollable-table'
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

const HEADERS = ['Value', 'Kind', 'List', 'Note', 'Created', 'By', 'Actions']
const NO_ROWS: GreylistRow[] = []

export function AdminGreylist() {
  const [filterKind, setFilterKind] = useState<'all' | GreylistLocalKind>('all')
  const [filterList, setFilterList] = useState<'all' | GreylistLocalListName>('all')
  const [adding, setAdding] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<null | number | string>(null)

  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.greylistLocal(),
    queryFn: ({ signal }) => adminListGet<GreylistRow>('/admin/greylist/local-lists', signal),
  })
  const entries = data ?? NO_ROWS

  const filtered = useMemo(
    () =>
      entries.filter(
        (e) =>
          (filterKind === 'all' || e.kind === filterKind) &&
          (filterList === 'all' || e.list === filterList)
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
    successMsg: (f) => `${f.list} ${f.kind} "${f.value.trim()}" added`,
  })

  const deleteEntry = useAdminMutation({
    invalidateKey: adminKeys.greylistLocal(),
    successMsg: 'Entry removed',
    mutationFn: (id: number | string) =>
      adminDelete(`/admin/greylist/local-lists/${encodeURIComponent(String(id))}`),
  })

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
          >
            Add Entry
          </button>
        )
      }
      title="Greylist Local Lists"
    >
      <div className="border-border mb-4 flex flex-wrap items-center gap-2 rounded-lg border p-3 text-sm">
        <label className="text-fg-muted text-xs">Kind</label>
        <select
          aria-label="Filter kind"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setFilterKind(e.target.value as 'all' | GreylistLocalKind)}
          value={filterKind}
        >
          <option value="all">all</option>
          <option value="domain">domain</option>
          <option value="email">email</option>
          <option value="cidr">cidr</option>
        </select>
        <label className="text-fg-muted ml-2 text-xs">List</label>
        <select
          aria-label="Filter list"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setFilterList(e.target.value as 'all' | GreylistLocalListName)}
          value={filterList}
        >
          <option value="all">all</option>
          <option value="white">white</option>
          <option value="black">black</option>
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

      <GreylistBody
        adding={adding}
        entries={entries}
        error={error}
        filtered={filtered}
        isPending={isPending}
        onDelete={setDeleteTarget}
        onRetry={() => refetch()}
      />

      {deleteTarget !== null && (
        <MobileModal onClose={() => setDeleteTarget(null)} open>
          <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
            <p className="text-fg-secondary mb-4 text-sm">
              Delete this greylist entry? Re-adding it later is fine; this only removes the current
              row.
            </p>
            <div className="flex justify-end gap-2">
              <button
                className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
                onClick={() => setDeleteTarget(null)}
              >
                Cancel
              </button>
              <button
                className="bg-danger rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
                disabled={deleteEntry.isPending}
                onClick={() =>
                  deleteEntry.mutate(deleteTarget, { onSettled: () => setDeleteTarget(null) })
                }
              >
                {busyLabel(deleteEntry.isPending, 'Delete', 'Deleting…')}
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}

function busyLabel(busy: boolean, idle: string, working: string): string {
  if (busy) return working
  return idle
}

function formatCreated(unix: number): string {
  if (!unix) return '—'
  const iso = new Date(unix * 1000).toISOString()
  return `${iso.slice(0, 10)} ${iso.slice(11, 16)}`
}

function GreylistBody({
  adding,
  entries,
  error,
  filtered,
  isPending,
  onDelete,
  onRetry,
}: {
  adding: boolean
  entries: GreylistRow[]
  error: Error | null
  filtered: GreylistRow[]
  isPending: boolean
  onDelete: (id: number | string) => void
  onRetry: () => void
}) {
  if (isPending) return <AdminTableSkeleton cols={HEADERS.length} headers={HEADERS} rows={5} />
  if (error) return <AdminErrorState error={error} onRetry={onRetry} />
  if (filtered.length === 0 && !adding) {
    let title = 'No entries match the filters'
    if (entries.length === 0) title = 'No local greylist entries'
    return (
      <AdminEmptyState
        description="Local lists override Phase 1's remote whitelist. White entries bypass greylisting; black entries are rejected with SMTP 550. Add your first entry to get started."
        icon={<Shield className="h-10 w-10" />}
        title={title}
      />
    )
  }
  return (
    <ScrollableTable>
      <table className="w-full text-left text-sm">
        <thead className="border-border bg-bg-secondary border-b">
          <tr>
            {HEADERS.map((h) => (
              <th className={thClass(h)} key={h}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {filtered.map((e) => (
            <tr className="border-border border-b last:border-0" key={String(e.id)}>
              <td className="px-4 py-3 font-mono text-xs">{e.value}</td>
              <td className="text-fg-secondary px-4 py-3">{e.kind}</td>
              <td className="px-4 py-3">
                <span
                  className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${listStyle(e.list)}`}
                >
                  {e.list}
                </span>
              </td>
              <td className="text-fg-secondary px-4 py-3 text-xs">{e.note ?? '—'}</td>
              <td className="text-fg-muted px-4 py-3 text-xs">{formatCreated(e.created_at)}</td>
              <td className="text-fg-muted px-4 py-3 text-xs">{e.created_by ?? '—'}</td>
              <td className="px-4 py-3 text-right">
                <button
                  className="text-danger text-xs transition-colors hover:opacity-70"
                  onClick={() => onDelete(e.id)}
                >
                  Delete
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollableTable>
  )
}

function listStyle(list: GreylistLocalListName): string {
  if (list === 'black') return 'bg-danger/10 text-danger'
  return 'bg-accent/10 text-accent'
}

function thClass(header: string): string {
  if (header === 'Actions') return 'px-4 py-2.5 text-right font-medium'
  return 'px-4 py-2.5 font-medium'
}
