import type { EmailGroupInfo } from '@/components/admin-email-groups/email-group-info'
import type { DomainInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Mailbox } from 'lucide-react'
import { useState } from 'react'

import { EmailGroupMembers } from '@/components/admin-email-groups/email-group-members'
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
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost } from '@/wire/endpoints/admin'

const HEADERS: Key[] = [
  'admin.col.address',
  'admin.col.domain',
  'admin.col.name',
  'admin.col.description',
]

const EMPTY_FORM = { address: '', description: '', domain: '', name: '' }

export function AdminEmailGroups() {
  const t = useT()
  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.emailGroups(),
    queryFn: ({ signal }) => adminListGet<EmailGroupInfo>('/admin/email-groups', signal),
  })
  const groups = data ?? []

  const { data: domainsData } = useQuery({
    queryKey: adminKeys.domains(),
    queryFn: ({ signal }) => adminListGet<DomainInfo>('/admin/domains', signal),
  })
  const domains = domainsData ?? []

  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<null | number>(null)
  const [deleteTarget, setDeleteTarget] = useState<EmailGroupInfo | null>(null)
  const [form, setForm] = useState(EMPTY_FORM)

  const addGroup = useAdminMutation({
    invalidateKey: adminKeys.emailGroups(),
    mutationFn: (vars: typeof form) =>
      adminPost('/admin/email-groups', {
        address: vars.address.trim(),
        description: vars.description.trim(),
        domain: vars.domain,
        name: vars.name.trim(),
      }),
    successMsg: (vars) => t('admin.emailGroups.added', { name: vars.name.trim() }),
  })

  const deleteGroup = useAdminMutation({
    invalidateKey: adminKeys.emailGroups(),
    successMsg: t('admin.emailGroups.deleted'),
    mutationFn: (id: number) => adminDelete(`/admin/email-groups/${id}`),
  })

  const formValid = !!form.address.trim() && !!form.domain && !!form.name.trim()

  const handleAdd = () => {
    if (!formValid) return
    addGroup.mutate(form, {
      onSuccess: () => {
        setForm(EMPTY_FORM)
        setAdding(false)
      },
    })
  }

  const handleDelete = (id: number) => {
    deleteGroup.mutate(id, {
      onSettled: () => {
        setDeleteTarget(null)
        if (expandedId === id) setExpandedId(null)
      },
    })
  }

  const toggle = (id: number) => {
    if (expandedId === id) setExpandedId(null)
    else setExpandedId(id)
  }

  const columns: Column<EmailGroupInfo>[] = [
    {
      header: t('admin.col.address'),
      key: 'address',
      cell: (g) => (
        <Copyable value={g.address}>
          <span className="font-medium">{g.address}</span>
        </Copyable>
      ),
    },
    {
      header: t('admin.col.domain'),
      key: 'domain',
      cell: (g) => <span className="text-fg-secondary">{g.domain}</span>,
    },
    {
      header: t('admin.col.name'),
      key: 'name',
      cell: (g) => <span className="text-fg-secondary">{g.name}</span>,
    },
    {
      header: t('admin.col.description'),
      key: 'description',
      cell: (g) => <span className="text-fg-secondary">{g.description}</span>,
    },
    {
      align: 'end',
      header: t('admin.col.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (g) => (
        <div className="flex justify-end gap-3 text-xs">
          <button
            className="text-accent hover:opacity-80"
            onClick={() => toggle(g.id)}
            type="button"
          >
            {t(membersLabel(expandedId === g.id))}
          </button>
          <button
            className="text-danger hover:opacity-70"
            onClick={() => setDeleteTarget(g)}
            type="button"
          >
            {t('admin.delete')}
          </button>
        </div>
      ),
    },
  ]

  const membersOf = (g: EmailGroupInfo) => {
    if (expandedId !== g.id) return null
    return <EmailGroupMembers group={g} />
  }

  const renderList = () => {
    if (isPending) {
      return <AdminTableSkeleton cols={5} headers={[...HEADERS.map((h) => t(h)), '']} rows={4} />
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          what={t('admin.emailGroups.listName')}
        />
      )
    }
    if (groups.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description={t('admin.emailGroups.emptyHelp')}
          icon={<Mailbox className="h-10 w-10" />}
          title={t('admin.emailGroups.empty')}
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={membersOf}
        label={t('admin.emailGroups.title')}
        rowKey={(g) => String(g.id)}
        rows={groups}
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
            {t('admin.emailGroups.add')}
          </button>
        )
      }
      title={t('admin.emailGroups.title')}
    >
      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="flex gap-2">
            <input
              aria-label={t('admin.emailAddress')}
              autoFocus
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, address: e.target.value })}
              placeholder="team@example.com"
              value={form.address}
            />
            <select
              aria-label={t('admin.col.domain')}
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              value={form.domain}
            >
              <option value="">{t('admin.selectDomain')}</option>
              {domains.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                </option>
              ))}
            </select>
          </div>
          <div className="flex gap-2">
            <input
              aria-label={t('admin.groups.name')}
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder={t('admin.groups.name')}
              value={form.name}
            />
            <input
              aria-label={t('admin.col.description')}
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, description: e.target.value })}
              placeholder={t('admin.col.description')}
              value={form.description}
            />
          </div>
          <div className="flex gap-2">
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
              disabled={!formValid || addGroup.isPending}
              onClick={handleAdd}
            >
              {t(pendingLabel(addGroup.isPending, 'admin.saving', 'admin.save'))}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm(EMPTY_FORM)
                setAdding(false)
              }}
            >
              {t('common.cancel')}
            </button>
          </div>
        </div>
      )}

      {renderList()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={t(
            pendingLabel(deleteGroup.isPending, 'admin.deleting', 'admin.emailGroups.delete')
          )}
          consequence={t('admin.emailGroups.deleteConsequence', { address: deleteTarget.address })}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget.id)}
          pending={deleteGroup.isPending}
          title={t('admin.deleteTitle', { name: deleteTarget.address })}
        />
      )}
    </AdminPageShell>
  )
}

function membersLabel(open: boolean): Key {
  if (open) return 'admin.hide'
  return 'admin.members'
}

function pendingLabel(pending: boolean, busy: Key, idle: Key): Key {
  if (pending) return busy
  return idle
}
