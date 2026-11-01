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
import { type Key, useT } from '@/i18n'
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

const HEADERS: Key[] = [
  'admin.greylist.value',
  'admin.col.type',
  'admin.greylist.list',
  'admin.greylist.note',
  'admin.domains.added',
  'admin.greylist.by',
]
const NO_ROWS: GreylistRow[] = []
const KINDS: GreylistLocalKind[] = ['domain', 'email', 'cidr']
const LISTS: GreylistLocalListName[] = ['white', 'black']
const KIND_LABELS: Record<GreylistLocalKind, Key> = {
  cidr: 'admin.greylist.kind.cidr',
  domain: 'admin.col.domain',
  email: 'admin.greylist.kind.email',
}
const LIST_LABELS: Record<GreylistLocalListName, Key> = {
  black: 'admin.greylist.black',
  white: 'admin.greylist.white',
}

// the list's name inside a sentence
const LIST_WORDS: Record<GreylistLocalListName, Key> = {
  black: 'admin.greylist.word.black',
  white: 'admin.greylist.word.white',
}

export function AdminGreylist() {
  const t = useT()
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
    successMsg: (f) =>
      t('admin.greylist.added', { list: t(LIST_WORDS[f.list]), value: f.value.trim() }),
  })

  const deleteEntry = useAdminMutation({
    invalidateKey: adminKeys.greylistLocal(),
    successMsg: t('admin.greylist.deleted'),
    mutationFn: (id: number | string) =>
      adminDelete(`/admin/greylist/local-lists/${encodeURIComponent(String(id))}`),
  })

  const columns: Column<GreylistRow>[] = [
    {
      header: t('admin.greylist.value'),
      key: 'value',
      cell: (e) => <span className="font-mono text-xs">{e.value}</span>,
    },
    {
      header: t('admin.col.type'),
      key: 'kind',
      cell: (e) => <span className="text-fg-secondary">{t(KIND_LABELS[e.kind])}</span>,
    },
    {
      header: t('admin.greylist.list'),
      key: 'list',
      cell: (e) => (
        <span className={`rounded px-2 py-0.5 text-xs font-medium ${listStyle(e.list)}`}>
          {t(LIST_LABELS[e.list])}
        </span>
      ),
    },
    {
      header: t('admin.greylist.note'),
      key: 'note',
      cell: (e) => <span className="text-fg-secondary text-xs">{e.note ?? '—'}</span>,
    },
    {
      align: 'end',
      header: t('admin.domains.added'),
      key: 'created',
      cell: (e) => <span className="text-fg-muted text-xs">{formatCreated(e.created_at)}</span>,
    },
    {
      header: t('admin.greylist.by'),
      key: 'by',
      cell: (e) => <span className="text-fg-muted text-xs">{e.created_by ?? '—'}</span>,
    },
    {
      align: 'end',
      header: t('admin.col.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (e) => (
        <button
          className="text-danger text-xs transition-colors hover:opacity-70"
          onClick={() => setDeleteTarget(e)}
          type="button"
        >
          {t('admin.delete')}
        </button>
      ),
    },
  ]

  const renderBody = () => {
    if (isPending) {
      return (
        <AdminTableSkeleton
          cols={HEADERS.length + 1}
          headers={[...HEADERS.map((h) => t(h)), '']}
          rows={5}
        />
      )
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          what={t('admin.greylist.listName')}
        />
      )
    }
    if (entries.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description={`${t('admin.greylist.emptyHelp')} ${t('admin.greylist.rules')}`}
          icon={<Shield className="h-10 w-10" />}
          title={t('admin.greylist.empty')}
        />
      )
    }
    if (filtered.length === 0) {
      return <p className="text-fg-muted py-8 text-center text-sm">{t('admin.audit.noMatch')}</p>
    }
    return (
      <AdminTable
        columns={columns}
        label={t('admin.greylist.entries')}
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
            {t('admin.greylist.add')}
          </button>
        )
      }
      subtitle={`${t('admin.greylist.subtitle')} ${t('admin.greylist.rules')}`}
      title={t('admin.greylist.title')}
    >
      <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
        <select
          aria-label={t('admin.greylist.filterKind')}
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setFilter('kind', e.target.value)}
          value={filterKind ?? 'all'}
        >
          <option value="all">{t('admin.greylist.allKinds')}</option>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {t(KIND_LABELS[k])}
            </option>
          ))}
        </select>
        <select
          aria-label={t('admin.greylist.filterList')}
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setFilter('list', e.target.value)}
          value={filterList ?? 'all'}
        >
          <option value="all">{t('admin.greylist.bothLists')}</option>
          {LISTS.map((l) => (
            <option key={l} value={l}>
              {t(LIST_LABELS[l])}
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
          confirmLabel={t(
            busyLabel(deleteEntry.isPending, 'admin.greylist.delete', 'admin.deleting')
          )}
          consequence={t(deleteConsequence(deleteTarget), { value: deleteTarget.value })}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() =>
            deleteEntry.mutate(deleteTarget.id, { onSettled: () => setDeleteTarget(null) })
          }
          pending={deleteEntry.isPending}
          title={t('admin.greylist.deleteTitle', {
            list: t(LIST_WORDS[deleteTarget.list]),
            value: deleteTarget.value,
          })}
        />
      )}
    </AdminPageShell>
  )
}

function busyLabel(busy: boolean, idle: Key, working: Key): Key {
  if (busy) return working
  return idle
}

function deleteConsequence(entry: GreylistRow): Key {
  if (entry.list === 'black') return 'admin.greylist.deleteBlack'
  return 'admin.greylist.deleteWhite'
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
