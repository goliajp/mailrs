import { useAtom } from 'jotai'
import { useCallback, useEffect, useState } from 'react'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { DomainInfo } from '@/lib/types'
import { domainsAtom } from '@/store/admin'

export function AdminDomains() {
  const [domains, setDomains] = useAtom(domainsAtom)
  const [adding, setAdding] = useState(false)
  const [newDomain, setNewDomain] = useState('')

  const loadDomains = useCallback(async () => {
    try {
      const data = await fetchJson<DomainInfo[]>('/admin/domains')
      setDomains(data)
    } catch {
      // keep current state on error
    }
  }, [setDomains])

  useEffect(() => {
    loadDomains()
  }, [loadDomains])

  const handleAdd = async () => {
    if (!newDomain.trim()) return
    await postJson('/admin/domains', { name: newDomain.trim() })
    setNewDomain('')
    setAdding(false)
    loadDomains()
  }

  const handleDelete = async (name: string) => {
    await deleteJson(`/admin/domains/${encodeURIComponent(name)}`)
    loadDomains()
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Domains</h2>
        <button
          onClick={() => setAdding(true)}
          className="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
        >
          Add Domain
        </button>
      </div>

      {adding && (
        <div className="mb-4 flex gap-2">
          <input
            value={newDomain}
            onChange={(e) => setNewDomain(e.target.value)}
            placeholder="example.com"
            className="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
            onKeyDown={(e) => e.key === 'Enter' && handleAdd()}
          />
          <button
            onClick={handleAdd}
            className="rounded-md bg-zinc-900 px-3 py-1.5 text-sm text-white dark:bg-zinc-100 dark:text-zinc-900"
          >
            Save
          </button>
          <button
            onClick={() => setAdding(false)}
            className="rounded-md px-3 py-1.5 text-sm text-zinc-500"
          >
            Cancel
          </button>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-800">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-zinc-200 bg-zinc-50 dark:border-zinc-800 dark:bg-zinc-900">
            <tr>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Created</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {domains.map((domain) => (
              <tr
                key={domain.name}
                className="border-b border-zinc-100 last:border-0 dark:border-zinc-800/50"
              >
                <td className="px-4 py-3 font-medium">{domain.name}</td>
                <td className="px-4 py-3 text-zinc-500">
                  {new Date(domain.created_at * 1000).toLocaleDateString()}
                </td>
                <td className="px-4 py-3 text-right">
                  <button
                    onClick={() => handleDelete(domain.name)}
                    className="text-xs text-red-500 hover:text-red-700"
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
            {domains.length === 0 && (
              <tr>
                <td colSpan={3} className="px-4 py-8 text-center text-zinc-400">
                  No domains configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}
