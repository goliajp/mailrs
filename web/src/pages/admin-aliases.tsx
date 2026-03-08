import { useAtom } from 'jotai'
import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { AliasInfo, DomainInfo } from '@/lib/types'
import { aliasesAtom, domainsAtom } from '@/store/admin'

export function AdminAliases() {
  const [aliases, setAliases] = useAtom(aliasesAtom)
  const [domains, setDomains] = useAtom(domainsAtom)
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({
    source_address: '',
    target_address: '',
    domain: '',
    alias_type: 'alias',
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
    if (!form.source_address.trim() || !form.target_address.trim() || !form.domain) return
    try {
      await postJson('/admin/aliases', {
        source_address: form.source_address.trim(),
        target_address: form.target_address.trim(),
        domain: form.domain,
        alias_type: form.alias_type,
      })
      toast.success(`Alias "${form.source_address.trim()}" added`)
      setForm({ source_address: '', target_address: '', domain: '', alias_type: 'alias' })
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
      loadAliases()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove alias')
    }
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Aliases</h2>
        <button
          onClick={() => setAdding(true)}
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
        >
          Add Alias
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              value={form.source_address}
              onChange={(e) => setForm({ ...form, source_address: e.target.value })}
              placeholder="admin@example.com"
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-1.5 text-sm"
            />
            <input
              value={form.target_address}
              onChange={(e) => setForm({ ...form, target_address: e.target.value })}
              placeholder="user@example.com"
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-1.5 text-sm"
            />
          </div>
          <div className="flex gap-2">
            <select
              value={form.domain}
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-1.5 text-sm"
            >
              <option value="">Select domain...</option>
              {domains.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                </option>
              ))}
            </select>
            <select
              value={form.alias_type}
              onChange={(e) => setForm({ ...form, alias_type: e.target.value })}
              className="w-36 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-1.5 text-sm"
            >
              <option value="alias">Alias</option>
              <option value="forward">Forward</option>
            </select>
          </div>
          <div className="flex gap-2">
            <button
              onClick={handleAdd}
              className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)]"
            >
              Save
            </button>
            <button
              onClick={() => setAdding(false)}
              className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
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
                key={alias.id}
                className="border-b border-[var(--color-border-default)] last:border-0"
              >
                <td className="px-4 py-3 font-medium">{alias.source_address}</td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)]">{alias.target_address}</td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)]">{alias.domain}</td>
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
                    onClick={() => handleDelete(alias.id)}
                    className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
            {aliases.length === 0 && (
              <tr>
                <td colSpan={5} className="px-4 py-8 text-center text-[var(--color-text-tertiary)]">
                  No aliases configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}
