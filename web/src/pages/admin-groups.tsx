import type { GroupInfo } from '@/components/admin-groups/group-info'

import { useQuery } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { useState } from 'react'

import { GroupDetail } from '@/components/admin-groups/group-detail'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
  ConfirmAction,
} from '@/components/admin-page'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost, adminStringListGet } from '@/wire/endpoints/admin'

const HEADERS: Key[] = ['admin.col.name', 'admin.col.type', 'admin.col.description']

const EMPTY_FORM = { description: '', name: '' }

export function AdminGroups() {
  const t = useT()
  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.groups(),
    queryFn: ({ signal }) => adminListGet<GroupInfo>('/admin/groups', signal),
  })
  const groups = data ?? []

  const { data: allPermissionsData } = useQuery({
    queryKey: adminKeys.permissions(),
    queryFn: ({ signal }) => adminStringListGet('/admin/permissions', signal),
  })
  const allPermissions = allPermissionsData ?? []

  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<null | number>(null)
  const [deleteTarget, setDeleteTarget] = useState<GroupInfo | null>(null)
  const [form, setForm] = useState(EMPTY_FORM)

  const addGroup = useAdminMutation({
    invalidateKey: adminKeys.groups(),
    mutationFn: (vars: { description: string; name: string }) => adminPost('/admin/groups', vars),
    successMsg: (vars) => t('admin.groups.added', { name: vars.name }),
  })

  const deleteGroup = useAdminMutation({
    invalidateKey: adminKeys.groups(),
    successMsg: t('admin.groups.deleted'),
    mutationFn: (id: number) => adminDelete(`/admin/groups/${id}`),
  })

  const handleAdd = () => {
    if (!form.name.trim()) return
    addGroup.mutate(
      {
        description: form.description.trim(),
        name: form.name.trim(),
      },
      {
        onSuccess: () => {
          setForm(EMPTY_FORM)
          setAdding(false)
        },
      }
    )
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

  const columns: Column<GroupInfo>[] = [
    {
      header: t('admin.col.name'),
      key: 'name',
      cell: (g) => <span className="font-medium">{g.name}</span>,
    },
    {
      header: t('admin.col.type'),
      key: 'type',
      cell: (g) => <span className="text-fg-secondary">{t(builtinLabel(g))}</span>,
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
            {t(manageLabel(expandedId === g.id))}
          </button>
          {!g.is_builtin && (
            <button
              className="text-danger hover:opacity-70"
              onClick={() => setDeleteTarget(g)}
              type="button"
            >
              {t('admin.delete')}
            </button>
          )}
        </div>
      ),
    },
  ]

  const detailOf = (g: GroupInfo) => {
    if (expandedId !== g.id) return null
    return <GroupDetail allPermissions={allPermissions} group={g} />
  }

  const renderList = () => {
    if (isPending) {
      return <AdminTableSkeleton cols={4} headers={[...HEADERS.map((h) => t(h)), '']} rows={4} />
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          what={t('admin.groups.listName')}
        />
      )
    }
    if (groups.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description={t('admin.groups.emptyHelp')}
          icon={<ShieldCheck className="h-10 w-10" />}
          title={t('admin.groups.empty')}
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={detailOf}
        label={t('admin.permissionGroups')}
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
            {t('admin.groups.add')}
          </button>
        )
      }
      subtitle={t('admin.groups.subtitle')}
      title={t('admin.permissionGroups')}
    >
      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <input
            aria-label={t('admin.groups.name')}
            autoFocus
            className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder={t('admin.groups.name')}
            value={form.name}
          />
          <input
            aria-label={t('admin.col.description')}
            className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setForm({ ...form, description: e.target.value })}
            placeholder={t('admin.col.description')}
            value={form.description}
          />
          <div className="flex gap-2">
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
              disabled={!form.name.trim() || addGroup.isPending}
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
            pendingLabel(deleteGroup.isPending, 'admin.deleting', 'admin.groups.delete')
          )}
          consequence={t('admin.groups.deleteConsequence')}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget.id)}
          pending={deleteGroup.isPending}
          title={t('admin.deleteTitle', { name: deleteTarget.name })}
        />
      )}
    </AdminPageShell>
  )
}

function builtinLabel(g: GroupInfo): Key {
  if (g.is_builtin) return 'admin.groups.builtin'
  return 'admin.groups.custom'
}

function manageLabel(open: boolean): Key {
  if (open) return 'admin.hide'
  return 'admin.groups.manage'
}

function pendingLabel(pending: boolean, busy: Key, idle: Key): Key {
  if (pending) return busy
  return idle
}
