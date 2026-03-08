import { useAtom } from 'jotai'
import { Fragment, useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { Copyable } from '@/components/copy-button'
import { DomainHealthCard } from '@/components/domain-health-card'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { DomainCheckReport, DomainInfo } from '@/lib/types'
import { domainsAtom } from '@/store/admin'

export function AdminDomains() {
  const [domains, setDomains] = useAtom(domainsAtom)
  const [adding, setAdding] = useState(false)
  const [newDomain, setNewDomain] = useState('')
  const [checking, setChecking] = useState<string | null>(null)
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
      loadDomains()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove domain')
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
          onClick={() => setAdding(true)}
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
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
            className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-1.5 text-sm"
            onKeyDown={(e) => e.key === 'Enter' && handleAdd()}
          />
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
      )}

      <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
            <tr>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Created</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {domains.map((domain) => (
              <Fragment key={domain.name}>
                <tr className="border-b border-[var(--color-border-default)] last:border-0">
                  <td className="px-4 py-3 font-medium"><Copyable value={domain.name}>{domain.name}</Copyable></td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {new Date(domain.created_at * 1000).toLocaleDateString()}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <button
                      onClick={() => toggleReport(domain.name)}
                      disabled={checking === domain.name}
                      className="mr-3 text-xs text-[var(--color-brand-primary)] hover:opacity-80 disabled:opacity-50"
                    >
                      {checking === domain.name
                        ? 'Checking...'
                        : reports[domain.name]
                          ? 'Hide'
                          : 'Check'}
                    </button>
                    <button
                      onClick={() => handleDelete(domain.name)}
                      className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
                    >
                      Delete
                    </button>
                  </td>
                </tr>
                {reports[domain.name] && (
                  <tr>
                    <td colSpan={3} className="px-4 pb-4 pt-1">
                      <DomainHealthCard
                        report={reports[domain.name]}
                        checking={checking === domain.name}
                        onRecheck={() => handleCheck(domain.name)}
                      />
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
            {domains.length === 0 && (
              <tr>
                <td colSpan={3} className="px-4 py-8 text-center text-[var(--color-text-tertiary)]">
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
