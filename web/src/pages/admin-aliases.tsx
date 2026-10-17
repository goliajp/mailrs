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

const HEADERS = ['Source', 'Target', 'Domain', '']

export function AdminAliases() {
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
    successMsg: (f) => `Alias "${f.source_address.trim()}" added`,
  })

  const deleteAlias = useAdminMutation({
    invalidateKey: adminKeys.aliases(),
    successMsg: 'Alias deleted',
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
      header: 'Source',
      key: 'source',
      cell: (a) => (
        <Copyable value={a.source_address}>
          <span className="font-medium">{a.source_address}</span>
        </Copyable>
      ),
    },
    {
      header: 'Target',
      key: 'target',
      cell: (a) => <span className="text-fg-secondary">{a.target_address}</span>,
    },
    {
      header: 'Domain',
      key: 'domain',
      cell: (a) => <span className="text-fg-secondary">{a.domain}</span>,
    },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (a) => (
        <button
          className="text-danger text-xs hover:opacity-70"
          onClick={() => setDeleteTarget(a)}
          type="button"
        >
          Delete
        </button>
      ),
    },
  ]

  const renderList = () => {
    if (isPending) return <AdminTableSkeleton cols={4} headers={HEADERS} rows={5} />
    if (error) {
      return <AdminErrorState error={error} onRetry={() => refetch()} what="the alias list" />
    }
    if (aliases.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description="An alias delivers mail sent to one address into another mailbox. A source of @example.com catches every address in that domain that has no mailbox or alias of its own"
          icon={<AtSign className="h-10 w-10" />}
          title="No aliases yet"
        />
      )
    }
    return <AdminTable columns={columns} label="Aliases" rowKey={(a) => a.id} rows={aliases} />
  }

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
          >
            Add alias
          </button>
        )
      }
      title="Aliases"
    >
      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="flex gap-2">
            <input
              aria-label="Source address"
              autoFocus
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, source_address: e.target.value })}
              placeholder="admin@example.com"
              value={form.source_address}
            />
            <input
              aria-label="Target address"
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
              {pendingLabel(addAlias.isPending, 'Saving...', 'Save')}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm(EMPTY_FORM)
                setAdding(false)
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {renderList()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={pendingLabel(deleteAlias.isPending, 'Deleting...', 'Delete alias')}
          consequence={aliasDeleteConsequence(deleteTarget)}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget.id)}
          pending={deleteAlias.isPending}
          title={`Delete ${deleteTarget.source_address}?`}
        />
      )}
    </AdminPageShell>
  )
}

/**
 * Delivery tries the address's own mailbox, then an exact alias, then the
 * domain's `@domain` catch-all, then an email group with that address.
 */
function aliasDeleteConsequence(alias: AliasRow): string {
  const { source_address: source, target_address: target } = alias
  if (source.startsWith('@')) {
    return `Mail to addresses at ${source.slice(1)} that have no mailbox or alias of their own stops going to ${target}. Mail already in ${target} is kept`
  }
  return `Mail to ${source} stops going to ${target}. If ${alias.domain} has a catch-all alias, that alias receives it instead. Mail already in ${target} is kept`
}

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}
