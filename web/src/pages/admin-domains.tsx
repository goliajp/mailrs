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
import { currentLocale, type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminObjectGet, adminPost } from '@/wire/endpoints/admin'

const HEADERS: Key[] = ['admin.col.domain', 'admin.domains.added']

type SystemConfigEntry = { key: string; value: null | string }

export function AdminDomains() {
  const t = useT()
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
  const steps = t('admin.domains.steps', {
    host: mailHost(systemConfig?.entries) ?? t('admin.domains.thisHost'),
  })

  const [adding, setAdding] = useState(false)
  const [newDomain, setNewDomain] = useState('')
  const [checking, setChecking] = useState<null | string>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
  const [reports, setReports] = useState<Record<string, DomainCheckReport>>({})

  const addDomain = useAdminMutation({
    invalidateKey: adminKeys.domains(),
    mutationFn: (name: string) => adminPost('/admin/domains', { name }),
    successMsg: (name) => t('admin.domains.addedToast', { name }),
  })

  const deleteDomain = useAdminMutation({
    invalidateKey: adminKeys.domains(),
    mutationFn: (name: string) => adminDelete(`/admin/domains/${encodeURIComponent(name)}`),
    successMsg: (name) => t('admin.domains.removedToast', { name }),
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
      header: t('admin.col.domain'),
      key: 'name',
      cell: (d) => (
        <Copyable value={d.name}>
          <span className="font-medium">{d.name}</span>
        </Copyable>
      ),
    },
    {
      header: t('admin.domains.added'),
      key: 'created',
      cell: (d) => (
        <span className="text-fg-secondary">
          {new Date(d.created_at * 1000).toLocaleDateString(currentLocale())}
        </span>
      ),
    },
    {
      align: 'end',
      header: t('admin.col.actions'),
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
            {t(checkLabel(checking === d.name, Boolean(reports[d.name])))}
          </button>
          <button
            className="text-danger hover:opacity-70"
            onClick={() => setDeleteTarget(d.name)}
            type="button"
          >
            {t('admin.delete')}
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
    if (isPending) {
      return <AdminTableSkeleton cols={3} headers={[...HEADERS.map((h) => t(h)), '']} rows={5} />
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          what={t('admin.domains.listName')}
        />
      )
    }
    if (domains.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description={steps}
          icon={<Globe className="h-10 w-10" />}
          title={t('admin.domains.empty')}
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={reportOf}
        label={t('admin.domains.title')}
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
            {t('admin.domains.add')}
          </button>
        )
      }
      subtitle={domainsSubtitle(domains.length, steps, t)}
      title={t('admin.domains.title')}
    >
      {adding && (
        <div className="mb-4 flex gap-2">
          <input
            aria-label={t('admin.domains.newName')}
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
            {t(pendingLabel(addDomain.isPending, 'admin.saving', 'admin.save'))}
          </button>
          <button
            className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
            onClick={() => setAdding(false)}
          >
            {t('common.cancel')}
          </button>
        </div>
      )}

      {renderList()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={t(
            pendingLabel(deleteDomain.isPending, 'admin.deleting', 'admin.domains.delete')
          )}
          consequence={t('admin.domains.deleteConsequence', { name: deleteTarget })}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
          pending={deleteDomain.isPending}
          title={t('admin.deleteTitle', { name: deleteTarget })}
        />
      )}
    </AdminPageShell>
  )
}

function checkLabel(running: boolean, shown: boolean): Key {
  if (running) return 'admin.domains.checking'
  if (shown) return 'admin.hide'
  return 'admin.domains.check'
}

function domainsSubtitle(
  count: number,
  steps: string,
  t: ReturnType<typeof useT>
): string | undefined {
  if (count === 0) return undefined
  return t('admin.domains.subtitle', { steps })
}

function mailHost(entries: SystemConfigEntry[] | undefined): string | undefined {
  const host = entries?.find((e) => e.key === 'hostname')?.value
  if (host) return host
  return undefined
}

function pendingLabel(pending: boolean, busy: Key, idle: Key): Key {
  if (pending) return busy
  return idle
}
