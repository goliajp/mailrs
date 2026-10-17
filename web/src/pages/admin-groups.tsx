import type { GroupInfo } from '@/components/admin-groups/group-info'
import type { DomainInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { Fragment, useState } from 'react'

import { GroupDetail } from '@/components/admin-groups/group-detail'
import { domainLabel } from '@/components/admin-groups/group-info'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { MobileModal } from '@/components/mobile-modal'
import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost, adminStringListGet } from '@/wire/endpoints/admin'

const HEADERS = ['Name', 'Domain', 'Builtin', 'Description', 'Actions']

export function AdminGroups() {
  const {
    data: groupsData,
    error,
    isPending,
    refetch,
  } = useQuery({
    queryKey: adminKeys.groups(),
    queryFn: ({ signal }) => adminListGet<GroupInfo>('/admin/groups', signal),
  })
  const groups = groupsData ?? []

  const { data: domainsData } = useQuery({
    queryKey: adminKeys.domains(),
    queryFn: ({ signal }) => adminListGet<DomainInfo>('/admin/domains', signal),
  })
  const domains = domainsData ?? []

  const { data: allPermissionsData } = useQuery({
    queryKey: adminKeys.permissions(),
    queryFn: ({ signal }) => adminStringListGet('/admin/permissions', signal),
  })
  const allPermissions = allPermissionsData ?? []

  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<null | number>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)
  const [form, setForm] = useState({
    description: '',
    domain: '',
    name: '',
  })

  const addGroup = useAdminMutation({
    invalidateKey: adminKeys.groups(),
    mutationFn: (vars: { description: string; domain: string | undefined; name: string }) =>
      adminPost('/admin/groups', vars),
    successMsg: (vars) => `Group "${vars.name}" added`,
  })

  const deleteGroup = useAdminMutation({
    invalidateKey: adminKeys.groups(),
    successMsg: 'Group removed',
    mutationFn: (id: number) => adminDelete(`/admin/groups/${id}`),
  })

  const handleAdd = () => {
    if (!form.name.trim()) return
    addGroup.mutate(
      {
        description: form.description.trim(),
        domain: form.domain || undefined,
        name: form.name.trim(),
      },
      {
        onSuccess: () => {
          setForm({ description: '', domain: '', name: '' })
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

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
          >
            Add Group
          </button>
        )
      }
      title="Groups"
    >
      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="flex gap-2">
            <input
              aria-label="Group name"
              autoFocus
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Group name"
              value={form.name}
            />
            <select
              aria-label="Domain"
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              value={form.domain}
            >
              <option value="">(Global)</option>
              {domains.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                </option>
              ))}
            </select>
          </div>
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
              {addGroup.isPending ? 'Saving...' : 'Save'}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm({ description: '', domain: '', name: '' })
                setAdding(false)
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {isPending ? (
        <AdminTableSkeleton cols={5} headers={HEADERS} rows={4} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} />
      ) : groups.length === 0 && !adding ? (
        <AdminEmptyState
          description="Groups bundle permissions and members for role-based access."
          icon={<ShieldCheck className="h-10 w-10" />}
          title="No groups configured"
        />
      ) : (
        <ScrollableTable>
          <table className="w-full text-left text-sm">
            <thead className="border-border bg-bg-secondary border-b">
              <tr>
                <th className="px-4 py-2.5 font-medium">Name</th>
                <th className="px-4 py-2.5 font-medium">Domain</th>
                <th className="px-4 py-2.5 font-medium">Builtin</th>
                <th className="px-4 py-2.5 font-medium">Description</th>
                <th className="px-4 py-2.5 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {groups.map((group) => (
                <Fragment key={group.id}>
                  <tr className="border-border border-b last:border-0">
                    <td className="px-4 py-3 font-medium">{group.name}</td>
                    <td className="text-fg-secondary px-4 py-3">{domainLabel(group.domain)}</td>
                    <td className="px-4 py-3">
                      {group.is_builtin && (
                        <span className="bg-surface text-fg-secondary inline-block rounded px-2 py-0.5 text-xs font-medium">
                          builtin
                        </span>
                      )}
                    </td>
                    <td className="text-fg-secondary px-4 py-3">{group.description}</td>
                    <td className="px-4 py-3 text-right">
                      <button
                        className="text-accent mr-3 text-xs hover:opacity-80"
                        onClick={() => setExpandedId(expandedId === group.id ? null : group.id)}
                      >
                        {expandedId === group.id ? 'Hide' : 'Manage'}
                      </button>
                      {!group.is_builtin && (
                        <button
                          className="text-danger text-xs transition-colors hover:opacity-70"
                          onClick={() => setDeleteTarget(group.id)}
                        >
                          Delete
                        </button>
                      )}
                    </td>
                  </tr>
                  {expandedId === group.id && (
                    <tr>
                      <td colSpan={5}>
                        <GroupDetail allPermissions={allPermissions} group={group} />
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        </ScrollableTable>
      )}

      {deleteTarget !== null && (
        <MobileModal onClose={() => setDeleteTarget(null)} open>
          <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
            <p className="text-fg-secondary mb-4 text-sm">
              Delete this group? This cannot be undone.
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
                disabled={deleteGroup.isPending}
                onClick={() => handleDelete(deleteTarget)}
              >
                {deleteGroup.isPending ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}
