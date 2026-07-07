import type {
  GreylistLocalEntry,
  GreylistLocalHealth,
  GreylistLocalKind,
  GreylistLocalListName,
} from '@/lib/types'

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
import { deleteJson, fetchJson, fetchList, postJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

type Form = {
  kind: GreylistLocalKind
  list: GreylistLocalListName
  note: string
  value: string
}

const EMPTY_FORM: Form = {
  kind: 'domain',
  list: 'white',
  note: '',
  value: '',
}

const HEADERS = ['Value', 'Kind', 'List', 'Note', 'Created', 'By', 'Actions']

const placeholderFor: Record<GreylistLocalKind, string> = {
  cidr: '203.0.113.0/24  or  2001:db8::/32',
  domain: 'gmail.com',
  email: 'user@example.com',
}

export function AdminGreylist() {
  const [filterKind, setFilterKind] = useState<'all' | GreylistLocalKind>('all')
  const [filterList, setFilterList] = useState<'all' | GreylistLocalListName>('all')
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState<Form>(EMPTY_FORM)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)
  const [formError, setFormError] = useState<null | string>(null)

  const {
    data: entriesData,
    error,
    isPending,
    refetch,
  } = useQuery({
    queryKey: adminKeys.greylistLocal(),
    queryFn: ({ signal }) => fetchList<GreylistLocalEntry>('/admin/greylist/local-lists', signal),
  })
  const entries = useMemo(() => entriesData ?? [], [entriesData])

  const { data: healthData } = useQuery({
    queryKey: adminKeys.greylistLocalHealth(),
    refetchInterval: 30_000,
    queryFn: ({ signal }) => fetchJson<{ greylist_local?: GreylistLocalHealth }>('/health', signal),
  })
  const health = healthData?.greylist_local

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
    mutationFn: (f: Form) =>
      postJson('/admin/greylist/local-lists', {
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
    mutationFn: (id: number) => deleteJson(`/admin/greylist/local-lists/${id}`),
  })

  const handleAdd = () => {
    const err = validateValue(form.kind, form.value)
    if (err) {
      setFormError(err)
      return
    }
    setFormError(null)
    addEntry.mutate(form, {
      onSuccess: () => {
        setForm(EMPTY_FORM)
        setAdding(false)
      },
    })
  }

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
      <div className="text-fg-secondary mb-3 flex flex-wrap items-center gap-3 text-xs">
        {health ? (
          <span>
            <span className="text-fg font-medium">{health.white + health.black}</span> live entries
            · <span className="text-fg font-medium">{health.white}</span> white ·{' '}
            <span className="text-fg font-medium">{health.black}</span> black
            {health.last_reload_at
              ? ` · last reloaded ${formatRelative(health.last_reload_at)}`
              : ' · never reloaded'}
            {health.last_error ? (
              <span className="text-danger ml-2">reload error: {health.last_error}</span>
            ) : null}
          </span>
        ) : (
          <span>loading status…</span>
        )}
      </div>

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
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="text-fg-secondary text-xs">
            <strong className="text-fg">white</strong> = skip greylist (Gmail/Outlook etc. already
            covered by remote whitelist). <strong className="text-fg">black</strong> = reject with
            SMTP 550 5.7.1. A value can be on either list, never both.
          </div>
          <div className="flex flex-wrap gap-2">
            <select
              aria-label="Kind"
              className="border-border bg-bg-secondary w-28 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, kind: e.target.value as GreylistLocalKind })}
              value={form.kind}
            >
              <option value="domain">domain</option>
              <option value="email">email</option>
              <option value="cidr">cidr</option>
            </select>
            <select
              aria-label="List"
              className="border-border bg-bg-secondary w-24 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, list: e.target.value as GreylistLocalListName })}
              value={form.list}
            >
              <option value="white">white</option>
              <option value="black">black</option>
            </select>
            <input
              aria-label="Value"
              autoFocus
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => {
                setForm({ ...form, value: e.target.value })
                setFormError(null)
              }}
              placeholder={placeholderFor[form.kind]}
              value={form.value}
            />
          </div>
          <input
            aria-label="Note (optional)"
            className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setForm({ ...form, note: e.target.value })}
            placeholder="optional note — why this entry exists"
            value={form.note}
          />
          {formError ? <p className="text-danger text-xs">{formError}</p> : null}
          <div className="flex gap-2">
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
              disabled={addEntry.isPending || !form.value.trim()}
              onClick={handleAdd}
            >
              {addEntry.isPending ? 'Saving…' : 'Save'}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm(EMPTY_FORM)
                setFormError(null)
                setAdding(false)
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {isPending ? (
        <AdminTableSkeleton cols={HEADERS.length} headers={HEADERS} rows={5} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} />
      ) : filtered.length === 0 && !adding ? (
        <AdminEmptyState
          description="Local lists override Phase 1's remote whitelist. White entries bypass greylisting; black entries are rejected with SMTP 550. Add your first entry to get started."
          icon={<Shield className="h-10 w-10" />}
          title={
            entries.length === 0 ? 'No local greylist entries' : 'No entries match the filters'
          }
        />
      ) : (
        <ScrollableTable>
          <table className="w-full text-left text-sm">
            <thead className="border-border bg-bg-secondary border-b">
              <tr>
                {HEADERS.map((h) => (
                  <th
                    className={
                      h === 'Actions'
                        ? 'px-4 py-2.5 text-right font-medium'
                        : 'px-4 py-2.5 font-medium'
                    }
                    key={h}
                  >
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {filtered.map((e) => (
                <tr className="border-border border-b last:border-0" key={e.id}>
                  <td className="px-4 py-3 font-mono text-xs">{e.value}</td>
                  <td className="text-fg-secondary px-4 py-3">{e.kind}</td>
                  <td className="px-4 py-3">
                    <span
                      className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                        e.list === 'black' ? 'bg-danger/10 text-danger' : 'bg-accent/10 text-accent'
                      }`}
                    >
                      {e.list}
                    </span>
                  </td>
                  <td className="text-fg-secondary px-4 py-3 text-xs">{e.note ?? '—'}</td>
                  <td className="text-fg-muted px-4 py-3 text-xs">
                    {formatRelative(e.created_at)}
                  </td>
                  <td className="text-fg-muted px-4 py-3 text-xs">{e.created_by ?? '—'}</td>
                  <td className="px-4 py-3 text-right">
                    <button
                      className="text-danger text-xs transition-colors hover:opacity-70"
                      onClick={() => setDeleteTarget(e.id)}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </ScrollableTable>
      )}

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
                onClick={() => {
                  if (deleteTarget !== null) {
                    deleteEntry.mutate(deleteTarget, {
                      onSettled: () => setDeleteTarget(null),
                    })
                  }
                }}
              >
                {deleteEntry.isPending ? 'Deleting…' : 'Delete'}
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}

function formatRelative(unix: number): string {
  const d = new Date(unix * 1000)
  return `${d.toISOString().slice(0, 10)} ${d.toISOString().slice(11, 16)}`
}

function validateValue(kind: GreylistLocalKind, value: string): null | string {
  const v = value.trim()
  if (!v) return 'value is required'
  if (kind === 'domain') {
    if (!v.includes('.') || v.includes('@') || v.includes(' ')) {
      return 'invalid domain (need at least one dot, no @, no spaces)'
    }
  } else if (kind === 'email') {
    if (!/^[^@\s]+@[^@\s.]+\.[^@\s]+$/.test(v)) {
      return 'invalid email (need user@host.tld)'
    }
  } else if (kind === 'cidr') {
    if (!v.includes('/')) return 'cidr needs a /N suffix'
  }
  return null
}
