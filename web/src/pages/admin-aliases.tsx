import type { AliasInfo, DomainInfo } from '@/lib/types'

import { useAtom } from 'jotai'
import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { aliasesAtom, domainsAtom } from '@/store/admin'

export function AdminAliases() {
  const [aliases, setAliases] = useAtom(aliasesAtom)
  const [domains, setDomains] = useAtom(domainsAtom)
  const [adding, setAdding] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)
  const [form, setForm] = useState({
    alias_type: 'alias',
    domain: '',
    source_address: '',
    target_address: '',
  })

  const loadAliases = useCallback(async () => {
    try {
      const data = await fetchJson<AliasInfo[]>('/admin/aliases')
      setAliases(data)
    } catch {
      // keep current state on error
    }
  }, [setAliases])

  const loadDomains = useCallback(async () => {
    try {
      const data = await fetchJson<DomainInfo[]>('/admin/domains')
      setDomains(data)
    } catch {
      // keep current state on error
    }
  }, [setDomains])

  useEffect(() => {
    loadAliases()
    loadDomains()
  }, [loadAliases, loadDomains])

  const handleAdd = async () => {
    if (
      !form.source_address.trim() ||
      !form.target_address.trim() ||
      !form.domain
    )
      return
    try {
      await postJson('/admin/aliases', {
        alias_type: form.alias_type,
        domain: form.domain,
        source_address: form.source_address.trim(),
        target_address: form.target_address.trim(),
      })
      toast.success(`Alias "${form.source_address.trim()}" added`)
      setForm({
        alias_type: 'alias',
        domain: '',
        source_address: '',
        target_address: '',
      })
      setAdding(false)
      loadAliases()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to add alias')
    }
  }

  const handleDelete = async (id: number) => {
    try {
      await deleteJson(`/admin/aliases/${id}`)
      toast.success('Alias removed')
      setDeleteTarget(null)
      loadAliases()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove alias')
      setDeleteTarget(null)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Aliases</h2>
        <button
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add Alias
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) =>
                setForm({ ...form, source_address: e.target.value })
              }
              placeholder="admin@example.com"
              value={form.source_address}
            />
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) =>
                setForm({ ...form, target_address: e.target.value })
              }
              placeholder="user@example.com"
              value={form.target_address}
            />
          </div>
          <div className="flex gap-2">
            <select
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
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
              className="w-36 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, alias_type: e.target.value })}
              value={form.alias_type}
            >
              <option value="alias">Alias</option>
              <option value="forward">Forward</option>
            </select>
          </div>
          <div className="flex gap-2">
            <button
              className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)]"
              onClick={handleAdd}
            >
              Save
            </button>
            <button
              className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
              onClick={() => setAdding(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
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
              <tr
                className="border-b border-[var(--color-border-default)] last:border-0"
                key={alias.id}
              >
                <td className="px-4 py-3 font-medium">
                  {alias.source_address}
                </td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                  {alias.target_address}
                </td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                  {alias.domain}
                </td>
                <td className="px-4 py-3">
                  <span
                    className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                      alias.alias_type === 'forward'
                        ? 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'
                        : 'bg-[var(--color-bg-raised)] text-[var(--color-text-secondary)]'
                    }`}
                  >
                    {alias.alias_type}
                  </span>
                </td>
                <td className="px-4 py-3 text-right">
                  <button
                    className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
                    onClick={() => setDeleteTarget(alias.id)}
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
            {aliases.length === 0 && (
              <tr>
                <td
                  className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
                  colSpan={5}
                >
                  No aliases configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {deleteTarget !== null && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-sm rounded-lg bg-[var(--color-bg-raised)] p-6 shadow-lg">
            <p className="mb-4 text-sm text-[var(--color-text-secondary)]">
              Delete this alias? This cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
                onClick={() => setDeleteTarget(null)}
              >
                Cancel
              </button>
              <button
                className="rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90"
                onClick={() => handleDelete(deleteTarget)}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
