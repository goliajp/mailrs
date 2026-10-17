import { useQuery } from '@tanstack/react-query'
import { AtSign } from 'lucide-react'
import { useState } from 'react'

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

const HEADERS = ['Source', 'Target', 'Domain', 'Actions']

export function AdminAliases() {
  const {
    data: aliasesData,
    error,
    isPending,
    refetch,
  } = useQuery({
    queryKey: adminKeys.aliases(),
    queryFn: ({ signal }) => adminListGet<AliasRow>('/admin/aliases', signal),
  })
  const aliases = aliasesData ?? []

  const [adding, setAdding] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
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
    successMsg: 'Alias removed',
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

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
          >
            Add Alias
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
              {addAlias.isPending ? 'Saving...' : 'Save'}
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

      {isPending ? (
        <AdminTableSkeleton cols={4} headers={HEADERS} rows={5} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} />
      ) : aliases.length === 0 && !adding ? (
        <AdminEmptyState
          description="Aliases forward mail from one address to another, or to a group."
          icon={<AtSign className="h-10 w-10" />}
          title="No aliases configured"
        />
      ) : (
        <ScrollableTable>
          <table className="w-full text-left text-sm">
            <thead className="border-border bg-bg-secondary border-b">
              <tr>
                <th className="px-4 py-2.5 font-medium">Source</th>
                <th className="px-4 py-2.5 font-medium">Target</th>
                <th className="px-4 py-2.5 font-medium">Domain</th>
                <th className="px-4 py-2.5 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {aliases.map((alias) => (
                <tr className="border-border border-b last:border-0" key={alias.id}>
                  <td className="px-4 py-3 font-medium">{alias.source_address}</td>
                  <td className="text-fg-secondary px-4 py-3">{alias.target_address}</td>
                  <td className="text-fg-secondary px-4 py-3">{alias.domain}</td>
                  <td className="px-4 py-3 text-right">
                    <button
                      className="text-danger text-xs transition-colors hover:opacity-70"
                      onClick={() => setDeleteTarget(alias.id)}
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
              Delete this alias? This cannot be undone.
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
                disabled={deleteAlias.isPending}
                onClick={() => handleDelete(deleteTarget)}
              >
                {deleteAlias.isPending ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}
