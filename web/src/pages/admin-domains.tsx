import type { DomainCheckReport, DomainInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Globe } from 'lucide-react'
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
import { DomainHealthCard } from '@/components/domain-health-card'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminObjectGet, adminPost } from '@/wire/endpoints/admin'

const HEADERS = ['Domain', 'Added', '']

type SystemConfigEntry = { key: string; value: null | string }

export function AdminDomains() {
  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.domains(),
    queryFn: ({ signal }) => adminListGet<DomainInfo>('/admin/domains', signal),
  })
  const domains = data ?? []
  // the MX target is the server's own hostname, which System Config shows
  const { data: systemConfig } = useQuery({
    queryKey: adminKeys.systemConfig(),
    queryFn: ({ signal }) =>
      adminObjectGet<{ entries?: SystemConfigEntry[] }>('/admin/system-config', signal),
  })
  const steps = setupSteps(mailHost(systemConfig?.entries))

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
        `/admin/domains/${encodeURIComponent(name)}/check`
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

  const columns: Column<DomainInfo>[] = [
    {
      header: 'Domain',
      key: 'name',
      cell: (d) => (
        <Copyable value={d.name}>
          <span className="font-medium">{d.name}</span>
        </Copyable>
      ),
    },
    {
      header: 'Added',
      key: 'created',
      cell: (d) => (
        <span className="text-fg-secondary">
          {new Date(d.created_at * 1000).toLocaleDateString()}
        </span>
      ),
    },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (d) => (
        <div className="flex justify-end gap-3 text-xs">
          <button
            className="text-accent hover:opacity-80 disabled:opacity-50"
            disabled={checking === d.name}
            onClick={() => toggleReport(d.name)}
            type="button"
          >
            {checkLabel(checking === d.name, Boolean(reports[d.name]))}
          </button>
          <button
            className="text-danger hover:opacity-70"
            onClick={() => setDeleteTarget(d.name)}
            type="button"
          >
            Delete
          </button>
        </div>
      ),
    },
  ]

  const reportOf = (d: DomainInfo) => {
    const report = reports[d.name]
    if (!report) return null
    return (
      <DomainHealthCard
        checking={checking === d.name}
        onRecheck={() => handleCheck(d.name)}
        report={report}
      />
    )
  }

  const renderList = () => {
    if (isPending) return <AdminTableSkeleton cols={3} headers={HEADERS} rows={5} />
    if (error) {
      return <AdminErrorState error={error} onRetry={() => refetch()} what="the domain list" />
    }
    if (domains.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description={steps}
          icon={<Globe className="h-10 w-10" />}
          title="No domains yet"
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={reportOf}
        label="Domains"
        rowKey={(d) => d.name}
        rows={domains}
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
            Add domain
          </button>
        )
      }
      subtitle={domainsSubtitle(domains.length, steps)}
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
            {pendingLabel(addDomain.isPending, 'Saving...', 'Save')}
          </button>
          <button
            className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
            onClick={() => setAdding(false)}
          >
            Cancel
          </button>
        </div>
      )}

      {renderList()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={pendingLabel(deleteDomain.isPending, 'Deleting...', 'Delete domain')}
          consequence={`${deleteTarget} leaves this list. Unless MAILRS_LOCAL_DOMAINS also names it, mail for it is refused within a minute. Its accounts, aliases and stored mail are kept`}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
          pending={deleteDomain.isPending}
          title={`Delete ${deleteTarget}?`}
        />
      )}
    </AdminPageShell>
  )
}

function checkLabel(running: boolean, shown: boolean): string {
  if (running) return 'Checking...'
  if (shown) return 'Hide'
  return 'Check'
}

function domainsSubtitle(count: number, steps: string): string | undefined {
  if (count === 0) return undefined
  return `${steps}. A domain added here is accepted within a minute`
}

function mailHost(entries: SystemConfigEntry[] | undefined): string {
  const host = entries?.find((e) => e.key === 'hostname')?.value
  if (host) return host
  return "this server's mail hostname"
}

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}

function setupSteps(host: string): string {
  return `To receive mail for a domain: add it here, point its MX record at ${host}, then use Check to verify SPF, DKIM, DMARC and MX`
}
