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
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost, adminStringListGet } from '@/wire/endpoints/admin'

const HEADERS = ['Name', 'Type', 'Description', '']

const EMPTY_FORM = { description: '', name: '' }

export function AdminGroups() {
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
    successMsg: (vars) => `Group "${vars.name}" added`,
  })

  const deleteGroup = useAdminMutation({
    invalidateKey: adminKeys.groups(),
    successMsg: 'Group deleted',
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
    { header: 'Name', key: 'name', cell: (g) => <span className="font-medium">{g.name}</span> },
    {
      header: 'Type',
      key: 'type',
      cell: (g) => <span className="text-fg-secondary">{builtinLabel(g)}</span>,
    },
    {
      header: 'Description',
      key: 'description',
      cell: (g) => <span className="text-fg-secondary">{g.description}</span>,
    },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (g) => (
        <div className="flex justify-end gap-3 text-xs">
          <button
            className="text-accent hover:opacity-80"
            onClick={() => toggle(g.id)}
            type="button"
          >
            {manageLabel(expandedId === g.id)}
          </button>
          {!g.is_builtin && (
            <button
              className="text-danger hover:opacity-70"
              onClick={() => setDeleteTarget(g)}
              type="button"
            >
              Delete
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
    if (isPending) return <AdminTableSkeleton cols={4} headers={HEADERS} rows={4} />
    if (error) {
      return (
        <AdminErrorState error={error} onRetry={() => refetch()} what="the permission group list" />
      )
    }
    if (groups.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description="A permission group lets several accounts share a set of permissions. Add a group, tick its permissions, then add its members"
          icon={<ShieldCheck className="h-10 w-10" />}
          title="No permission groups yet"
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={detailOf}
        label="Permission groups"
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
            Add group
          </button>
        )
      }
      subtitle="A member gets every permission of every group it belongs to, on top of its own, on every domain"
      title="Permission groups"
    >
      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <input
            aria-label="Group name"
            autoFocus
            className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="Group name"
            value={form.name}
          />
          <input
            aria-label="Description"
            className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setForm({ ...form, description: e.target.value })}
            placeholder="Description"
            value={form.description}
          />
          <div className="flex gap-2">
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
              disabled={!form.name.trim() || addGroup.isPending}
              onClick={handleAdd}
            >
              {pendingLabel(addGroup.isPending, 'Saving...', 'Save')}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm(EMPTY_FORM)
                setAdding(false)
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {renderList()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={pendingLabel(deleteGroup.isPending, 'Deleting...', 'Delete group')}
          consequence="The group, its permission list and its member list are deleted. Its members lose the permissions it granted, unless their own account or another group grants them too. The member accounts are kept"
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget.id)}
          pending={deleteGroup.isPending}
          title={`Delete ${deleteTarget.name}?`}
        />
      )}
    </AdminPageShell>
  )
}

function builtinLabel(g: GroupInfo): string {
  if (g.is_builtin) return 'Built-in'
  return 'Custom'
}

function manageLabel(open: boolean): string {
  if (open) return 'Hide'
  return 'Manage'
}

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}
