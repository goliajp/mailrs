import type { AliasInfo, DomainInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { MobileModal } from '@/components/mobile-modal'
import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

const EMPTY_FORM = {
  alias_type: 'alias',
  domain: '',
  source_address: '',
  target_address: '',
}

type AliasForm = typeof EMPTY_FORM

export function AdminAliases() {
  const { data: aliases = [] } = useQuery({
    queryKey: adminKeys.aliases(),
    queryFn: ({ signal }) => fetchJson<AliasInfo[]>('/admin/aliases', signal),
  })
  const { data: domains = [] } = useQuery({
    queryKey: adminKeys.domains(),
    queryFn: ({ signal }) => fetchJson<DomainInfo[]>('/admin/domains', signal),
  })
  const [adding, setAdding] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)
  const [form, setForm] = useState<AliasForm>(EMPTY_FORM)

  const addAlias = useAdminMutation({
    invalidateKey: adminKeys.aliases(),
    mutationFn: (f: AliasForm) =>
      postJson('/admin/aliases', {
        alias_type: f.alias_type,
        domain: f.domain,
        source_address: f.source_address.trim(),
        target_address: f.target_address.trim(),
      }),
    successMsg: (f) => `Alias "${f.source_address.trim()}" added`,
  })

  const deleteAlias = useAdminMutation({
    invalidateKey: adminKeys.aliases(),
    successMsg: 'Alias removed',
    mutationFn: (id: number) => deleteJson(`/admin/aliases/${id}`),
  })

  const handleAdd = () => {
    if (!form.source_address.trim() || !form.target_address.trim() || !form.domain) return
    addAlias.mutate(form, {
      onSuccess: () => {
        setForm(EMPTY_FORM)
        setAdding(false)
      },
    })
  }

  const handleDelete = (id: number) => {
    deleteAlias.mutate(id, {
      onSettled: () => setDeleteTarget(null),
    })
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Aliases</h2>
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add Alias
        </button>
      </div>

      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="flex gap-2">
            <input
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, source_address: e.target.value })}
              placeholder="admin@example.com"
              value={form.source_address}
            />
            <input
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, target_address: e.target.value })}
              placeholder="user@example.com"
              value={form.target_address}
            />
          </div>
          <div className="flex gap-2">
            <select
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              value={form.domain}
            >
              <option value="">Select domain...</option>
              {domains.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                </option>
              ))}
            </select>
            <select
              className="border-border bg-bg-secondary w-36 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, alias_type: e.target.value })}
              value={form.alias_type}
            >
              <option value="alias">Alias</option>
              <option value="forward">Forward</option>
            </select>
          </div>
          <div className="flex gap-2">
            <button className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm" onClick={handleAdd}>
              Save
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => setAdding(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <ScrollableTable>
        <table className="w-full text-left text-sm">
          <thead className="border-border bg-bg-secondary border-b">
            <tr>
              <th className="px-4 py-2.5 font-medium">Source</th>
              <th className="px-4 py-2.5 font-medium">Target</th>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Type</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {aliases.map((alias) => (
              <tr className="border-border border-b last:border-0" key={alias.id}>
                <td className="px-4 py-3 font-medium">{alias.source_address}</td>
                <td className="text-fg-secondary px-4 py-3">{alias.target_address}</td>
                <td className="text-fg-secondary px-4 py-3">{alias.domain}</td>
                <td className="px-4 py-3">
                  <span
                    className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                      alias.alias_type === 'forward'
                        ? 'bg-accent/10 text-accent'
                        : 'bg-surface text-fg-secondary'
                    }`}
                  >
                    {alias.alias_type}
                  </span>
                </td>
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
            {aliases.length === 0 && (
              <tr>
                <td className="text-fg-muted px-4 py-8 text-center" colSpan={5}>
                  No aliases configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </ScrollableTable>

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
                className="bg-danger rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90"
                onClick={() => handleDelete(deleteTarget)}
              >
                Delete
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </div>
  )
}
