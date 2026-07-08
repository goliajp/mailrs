import type { DomainCheckReport, DomainInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Globe } from 'lucide-react'
import { Fragment, useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { Copyable } from '@/components/copy-button'
import { DomainHealthCard } from '@/components/domain-health-card'
import { MobileModal } from '@/components/mobile-modal'
import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost } from '@/wire/endpoints/admin'

const HEADERS = ['Domain', 'Created', 'Actions']

export function AdminDomains() {
  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.domains(),
    queryFn: ({ signal }) => adminListGet<DomainInfo>('/admin/domains', signal),
  })
  const domains = data ?? []

  const [adding, setAdding] = useState(false)
  const [newDomain, setNewDomain] = useState('')
  const [checking, setChecking] = useState<null | string>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
  const [reports, setReports] = useState<Record<string, DomainCheckReport>>({})

  const addDomain = useAdminMutation({
    invalidateKey: adminKeys.domains(),
    mutationFn: (name: string) => adminPost('/admin/domains', { name }),
    successMsg: (name) => `Domain "${name}" added`,
  })

  const deleteDomain = useAdminMutation({
    invalidateKey: adminKeys.domains(),
    mutationFn: (name: string) => adminDelete(`/admin/domains/${encodeURIComponent(name)}`),
    successMsg: (name) => `Domain "${name}" removed`,
  })

  const handleAdd = () => {
    const name = newDomain.trim()
    if (!name) return
    addDomain.mutate(name, {
      onSuccess: () => {
        setNewDomain('')
        setAdding(false)
      },
    })
  }

  const handleDelete = (name: string) => {
    deleteDomain.mutate(name, {
      onSettled: () => setDeleteTarget(null),
    })
  }

  const handleCheck = async (name: string) => {
    setChecking(name)
    try {
      const report = await adminPost<DomainCheckReport>(
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
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
          >
            Add Domain
          </button>
        )
      }
      title="Domains"
    >
      {adding && (
        <div className="mb-4 flex gap-2">
          <input
            aria-label="New domain name"
            autoFocus
            className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setNewDomain(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleAdd()
              if (e.key === 'Escape') setAdding(false)
            }}
            placeholder="example.com"
            value={newDomain}
          />
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
            disabled={addDomain.isPending || !newDomain.trim()}
            onClick={handleAdd}
          >
            {addDomain.isPending ? 'Saving...' : 'Save'}
          </button>
          <button
            className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
            onClick={() => setAdding(false)}
          >
            Cancel
          </button>
        </div>
      )}

      {isPending ? (
        <AdminTableSkeleton cols={3} headers={HEADERS} rows={5} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} />
      ) : domains.length === 0 && !adding ? (
        <AdminEmptyState
          description="Add a domain to start receiving mail for it."
          icon={<Globe className="h-10 w-10" />}
          title="No domains configured"
        />
      ) : (
        <ScrollableTable>
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
            </tbody>
          </table>
        </ScrollableTable>
      )}

      {deleteTarget && (
        <MobileModal onClose={() => setDeleteTarget(null)} open>
          <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
            <p className="text-fg-secondary mb-4 text-sm">
              Delete domain <span className="text-fg font-medium">{deleteTarget}</span>? This will
              also remove all associated accounts and aliases.
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
                disabled={deleteDomain.isPending}
                onClick={() => handleDelete(deleteTarget)}
              >
                {deleteDomain.isPending ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}
