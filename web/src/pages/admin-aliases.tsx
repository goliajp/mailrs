import { useQuery } from '@tanstack/react-query'
import { AtSign } from 'lucide-react'
import { useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
  ConfirmAction,
} from '@/components/admin-page'
import { Copyable } from '@/components/copy-button'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost } from '@/wire/endpoints/admin'

const EMPTY_FORM = { source_address: '', target_address: '' }

type AliasForm = typeof EMPTY_FORM

/**
 * What the alias store holds: a pair, keyed by its source address. It
 * keeps no type or creation time, so the page shows neither.
 * Backend: crates/webapi/src/handlers/admin_directory.rs — fn list_aliases
 */
type AliasRow = {
  domain: string
  id: string
  source_address: string
  target_address: string
}

const HEADERS: Key[] = ['admin.aliases.source', 'admin.aliases.target', 'admin.col.domain']

export function AdminAliases() {
  const t = useT()
  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.aliases(),
    queryFn: ({ signal }) => adminListGet<AliasRow>('/admin/aliases', signal),
  })
  const aliases = data ?? []

  const [adding, setAdding] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<AliasRow | null>(null)
  const [form, setForm] = useState<AliasForm>(EMPTY_FORM)

  const addAlias = useAdminMutation({
    invalidateKey: adminKeys.aliases(),
    mutationFn: (f: AliasForm) =>
      adminPost('/admin/aliases', {
        source_address: f.source_address.trim(),
        target_address: f.target_address.trim(),
      }),
    successMsg: (f) => t('admin.aliases.added', { address: f.source_address.trim() }),
  })

  const deleteAlias = useAdminMutation({
    invalidateKey: adminKeys.aliases(),
    successMsg: t('admin.aliases.deleted'),
    mutationFn: (id: string) => adminDelete(`/admin/aliases/${encodeURIComponent(id)}`),
  })

  const formValid = !!form.source_address.trim() && !!form.target_address.trim()

  const handleAdd = () => {
    if (!formValid) return
    addAlias.mutate(form, {
      onSuccess: () => {
        setForm(EMPTY_FORM)
        setAdding(false)
      },
    })
  }

  const handleDelete = (id: string) => {
    deleteAlias.mutate(id, {
      onSettled: () => setDeleteTarget(null),
    })
  }

  const columns: Column<AliasRow>[] = [
    {
      header: t('admin.aliases.source'),
      key: 'source',
      cell: (a) => (
        <Copyable value={a.source_address}>
          <span className="font-medium">{a.source_address}</span>
        </Copyable>
      ),
    },
    {
      header: t('admin.aliases.target'),
      key: 'target',
      cell: (a) => <span className="text-fg-secondary">{a.target_address}</span>,
    },
    {
      header: t('admin.col.domain'),
      key: 'domain',
      cell: (a) => <span className="text-fg-secondary">{a.domain}</span>,
    },
    {
      align: 'end',
      header: t('admin.col.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (a) => (
        <button
          className="text-danger text-xs hover:opacity-70"
          onClick={() => setDeleteTarget(a)}
          type="button"
        >
          {t('admin.delete')}
        </button>
      ),
    },
  ]

  const renderList = () => {
    if (isPending) {
      return <AdminTableSkeleton cols={4} headers={[...HEADERS.map((h) => t(h)), '']} rows={5} />
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          what={t('admin.aliases.listName')}
        />
      )
    }
    if (aliases.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description={t('admin.aliases.emptyHelp')}
          icon={<AtSign className="h-10 w-10" />}
          title={t('admin.aliases.empty')}
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        label={t('admin.aliases.title')}
        rowKey={(a) => a.id}
        rows={aliases}
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
          >
            {t('admin.aliases.add')}
          </button>
        )
      }
      title={t('admin.aliases.title')}
    >
      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="flex gap-2">
            <input
              aria-label={t('admin.aliases.sourceAddress')}
              autoFocus
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, source_address: e.target.value })}
              placeholder="admin@example.com"
              value={form.source_address}
            />
            <input
              aria-label={t('admin.aliases.targetAddress')}
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, target_address: e.target.value })}
              placeholder="user@example.com"
              value={form.target_address}
            />
          </div>
          <div className="flex gap-2">
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
              disabled={!formValid || addAlias.isPending}
              onClick={handleAdd}
            >
              {t(pendingLabel(addAlias.isPending, 'admin.saving', 'admin.save'))}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm(EMPTY_FORM)
                setAdding(false)
              }}
            >
              {t('common.cancel')}
            </button>
          </div>
        </div>
      )}

      {renderList()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={t(
            pendingLabel(deleteAlias.isPending, 'admin.deleting', 'admin.aliases.delete')
          )}
          consequence={aliasDeleteConsequence(deleteTarget, t)}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget.id)}
          pending={deleteAlias.isPending}
          title={t('admin.deleteTitle', { name: deleteTarget.source_address })}
        />
      )}
    </AdminPageShell>
  )
}

/**
 * Delivery tries the address's own mailbox, then an exact alias, then the
 * domain's `@domain` catch-all, then an email group with that address.
 */
function aliasDeleteConsequence(alias: AliasRow, t: ReturnType<typeof useT>): string {
  const { source_address: source, target_address: target } = alias
  if (source.startsWith('@')) {
    return t('admin.aliases.deleteCatchAll', { domain: source.slice(1), target })
  }
  return t('admin.aliases.deleteOne', { domain: alias.domain, source, target })
}

function pendingLabel(pending: boolean, busy: Key, idle: Key): Key {
  if (pending) return busy
  return idle
}
