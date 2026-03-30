import type { DomainCheckReport, DomainInfo } from '@/lib/types'

import { toast } from '@goliapkg/gds'
import { useAtom } from 'jotai'
import { Fragment, useCallback, useEffect, useState } from 'react'

import { Copyable } from '@/components/copy-button'
import { DomainHealthCard } from '@/components/domain-health-card'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { domainsAtom } from '@/store/admin'

export function AdminDomains() {
  const [domains, setDomains] = useAtom(domainsAtom)
  const [adding, setAdding] = useState(false)
  const [newDomain, setNewDomain] = useState('')
  const [checking, setChecking] = useState<null | string>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
  const [reports, setReports] = useState<Record<string, DomainCheckReport>>({})

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
    try {
      await postJson('/admin/domains', { name: newDomain.trim() })
      toast.success(`Domain "${newDomain.trim()}" added`)
      setNewDomain('')
      setAdding(false)
      loadDomains()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to add domain')
    }
  }

  const handleDelete = async (name: string) => {
    try {
      await deleteJson(`/admin/domains/${encodeURIComponent(name)}`)
      toast.success(`Domain "${name}" removed`)
      setDeleteTarget(null)
      loadDomains()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove domain')
      setDeleteTarget(null)
    }
  }

  const handleCheck = async (name: string) => {
    setChecking(name)
    try {
      const report = await postJson<DomainCheckReport>(
        `/admin/domains/${encodeURIComponent(name)}/check`,
        {}
      )
      setReports((prev) => ({ ...prev, [name]: report }))
    } catch {
      // keep any previous report
    } finally {
      setChecking(null)
    }
  }

  const toggleReport = (name: string) => {
    if (reports[name]) {
      setReports((prev) => {
        const next = { ...prev }
        delete next[name]
        return next
      })
    } else {
      handleCheck(name)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Domains</h2>
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add Domain
        </button>
      </div>

      {adding && (
        <div className="mb-4 flex gap-2">
          <input
            className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setNewDomain(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleAdd()}
            placeholder="example.com"
            value={newDomain}
          />
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm"
            onClick={handleAdd}
          >
            Save
          </button>
          <button
            className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
            onClick={() => setAdding(false)}
          >
            Cancel
          </button>
        </div>
      )}

      <div className="border-border overflow-hidden rounded-lg border">
        <table className="w-full text-left text-sm">
          <thead className="border-border bg-bg-secondary border-b">
            <tr>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Created</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {domains.map((domain) => (
              <Fragment key={domain.name}>
                <tr className="border-border border-b last:border-0">
                  <td className="px-4 py-3 font-medium">
                    <Copyable value={domain.name}>{domain.name}</Copyable>
                  </td>
                  <td className="text-fg-secondary px-4 py-3">
                    {new Date(domain.created_at * 1000).toLocaleDateString()}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <button
                      className="text-accent mr-3 text-xs hover:opacity-80 disabled:opacity-50"
                      disabled={checking === domain.name}
                      onClick={() => toggleReport(domain.name)}
                    >
                      {checking === domain.name
                        ? 'Checking...'
                        : reports[domain.name]
                          ? 'Hide'
                          : 'Check'}
                    </button>
                    <button
                      className="text-danger text-xs transition-colors hover:opacity-70"
                      onClick={() => setDeleteTarget(domain.name)}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
                {reports[domain.name] && (
                  <tr>
                    <td className="px-4 pt-1 pb-4" colSpan={3}>
                      <DomainHealthCard
                        checking={checking === domain.name}
                        onRecheck={() => handleCheck(domain.name)}
                        report={reports[domain.name]}
                      />
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
            {domains.length === 0 && (
              <tr>
                <td className="text-fg-muted px-4 py-8 text-center" colSpan={3}>
                  No domains configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {deleteTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
            <p className="text-fg-secondary mb-4 text-sm">
              Delete domain{' '}
              <span className="text-fg font-medium">{deleteTarget}</span>? This
              will also remove all associated accounts and aliases.
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
        </div>
      )}
    </div>
  )
}
