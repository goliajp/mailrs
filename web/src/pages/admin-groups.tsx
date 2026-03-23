import type { DomainInfo } from '@/lib/types'

import { Fragment, useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson, putJson } from '@/lib/api'

type ExpandedData = {
  members: string[]
  permissions: string[]
}

type GroupInfo = {
  description: string
  domain: null | string
  id: number
  is_builtin: boolean
  name: string
}

export function AdminGroups() {
  const [groups, setGroups] = useState<GroupInfo[]>([])
  const [domains, setDomains] = useState<DomainInfo[]>([])
  const [allPermissions, setAllPermissions] = useState<string[]>([])
  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<null | number>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)
  const [form, setForm] = useState({
    description: '',
    domain: '',
    name: '',
  })

  const loadGroups = useCallback(async () => {
    try {
      const data = await fetchJson<GroupInfo[]>('/admin/groups')
      setGroups(data)
    } catch {
      // keep current state
    }
  }, [])

  const loadDomains = useCallback(async () => {
    try {
      const data = await fetchJson<DomainInfo[]>('/admin/domains')
      setDomains(data)
    } catch {
      // keep current state
    }
  }, [])

  const loadPermissions = useCallback(async () => {
    try {
      const data = await fetchJson<string[]>('/admin/permissions')
      setAllPermissions(data)
    } catch {
      // keep current state
    }
  }, [])

  useEffect(() => {
    void loadGroups()
    void loadDomains()
    void loadPermissions()
  }, [loadGroups, loadDomains, loadPermissions])

  const handleAdd = async () => {
    if (!form.name.trim()) return
    try {
      await postJson('/admin/groups', {
        description: form.description.trim(),
        domain: form.domain || undefined,
        name: form.name.trim(),
      })
      toast.success(`Group "${form.name.trim()}" added`)
      setForm({ description: '', domain: '', name: '' })
      setAdding(false)
      loadGroups()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to add group')
    }
  }

  const handleDelete = async (id: number) => {
    try {
      await deleteJson(`/admin/groups/${id}`)
      toast.success('Group removed')
      setDeleteTarget(null)
      if (expandedId === id) setExpandedId(null)
      loadGroups()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove group')
      setDeleteTarget(null)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Groups</h2>
        <button
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add Group
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Group name"
              value={form.name}
            />
            <select
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
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
            className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            onChange={(e) => setForm({ ...form, description: e.target.value })}
            placeholder="Description"
            value={form.description}
          />
          <div className="flex gap-2">
            <button
              className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)]"
              onClick={handleAdd}
            >
              Save
            </button>
            <button
              className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
              onClick={() => setAdding(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
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
                <tr className="border-b border-[var(--color-border-default)] last:border-0">
                  <td className="px-4 py-3 font-medium">{group.name}</td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {group.domain ?? '(Global)'}
                  </td>
                  <td className="px-4 py-3">
                    {group.is_builtin && (
                      <span className="inline-block rounded bg-[var(--color-bg-raised)] px-2 py-0.5 text-xs font-medium text-[var(--color-text-secondary)]">
                        builtin
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {group.description}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <button
                      className="mr-3 text-xs text-[var(--color-brand-primary)] hover:opacity-80"
                      onClick={() =>
                        setExpandedId(expandedId === group.id ? null : group.id)
                      }
                    >
                      {expandedId === group.id ? 'Hide' : 'Manage'}
                    </button>
                    {!group.is_builtin && (
                      <button
                        className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
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
                      <GroupDetail
                        allPermissions={allPermissions}
                        group={group}
                        onChanged={loadGroups}
                      />
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
            {groups.length === 0 && (
              <tr>
                <td
                  className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
                  colSpan={5}
                >
                  No groups configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {deleteTarget !== null && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-sm rounded-lg bg-[var(--color-bg-raised)] p-6 shadow-lg">
            <p className="mb-4 text-sm text-[var(--color-text-secondary)]">
              Delete this group? This cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
                onClick={() => setDeleteTarget(null)}
              >
                Cancel
              </button>
              <button
                className="rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90"
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

function GroupDetail({
  allPermissions,
  group,
  onChanged,
}: {
  allPermissions: string[]
  group: GroupInfo
  onChanged: () => void
}) {
  const [data, setData] = useState<ExpandedData | null>(null)
  const [newMember, setNewMember] = useState('')

  const load = useCallback(async () => {
    try {
      const [perms, members] = await Promise.all([
        fetchJson<string[]>(`/admin/groups/${group.id}/permissions`),
        fetchJson<string[]>(`/admin/groups/${group.id}/members`),
      ])
      setData({ members, permissions: perms })
    } catch {
      // keep current state
    }
  }, [group.id])

  useEffect(() => {
    void load()
  }, [load])

  const handleTogglePermission = async (perm: string, checked: boolean) => {
    if (!data) return
    const updated = checked
      ? [...data.permissions, perm]
      : data.permissions.filter((p) => p !== perm)
    try {
      await putJson(`/admin/groups/${group.id}/permissions`, {
        permissions: updated,
      })
      setData({ ...data, permissions: updated })
      toast.success('Permissions updated')
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : 'Failed to update permissions'
      )
    }
  }

  const handleAddMember = async () => {
    if (!newMember.trim()) return
    try {
      await postJson(`/admin/groups/${group.id}/members`, {
        address: newMember.trim(),
      })
      toast.success(`Member "${newMember.trim()}" added`)
      setNewMember('')
      load()
      onChanged()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to add member')
    }
  }

  const handleRemoveMember = async (address: string) => {
    if (!window.confirm(`Remove member "${address}" from this group?`)) return
    try {
      await deleteJson(
        `/admin/groups/${group.id}/members/${encodeURIComponent(address)}`
      )
      toast.success(`Member "${address}" removed`)
      load()
      onChanged()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove member')
    }
  }

  if (!data) {
    return (
      <div className="px-4 py-3 text-sm text-[var(--color-text-tertiary)]">
        Loading...
      </div>
    )
  }

  return (
    <div className="space-y-4 px-4 pt-1 pb-4">
      <div>
        <h4 className="mb-2 text-xs font-medium text-[var(--color-text-secondary)]">
          Permissions
        </h4>
        <div className="flex flex-wrap gap-2">
          {allPermissions.map((perm) => (
            <label
              className="flex items-center gap-1.5 rounded px-2 py-1 text-xs hover:bg-[var(--color-hover)]"
              key={perm}
            >
              <input
                checked={data.permissions.includes(perm)}
                onChange={(e) => handleTogglePermission(perm, e.target.checked)}
                type="checkbox"
              />
              {perm}
            </label>
          ))}
          {allPermissions.length === 0 && (
            <span className="text-xs text-[var(--color-text-tertiary)]">
              No permissions available
            </span>
          )}
        </div>
      </div>

      <div>
        <h4 className="mb-2 text-xs font-medium text-[var(--color-text-secondary)]">
          Members
        </h4>
        <div className="mb-2 flex gap-2">
          <input
            className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            onChange={(e) => setNewMember(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleAddMember()}
            placeholder="user@example.com"
            value={newMember}
          />
          <button
            className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)]"
            onClick={handleAddMember}
          >
            Add
          </button>
        </div>
        {data.members.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {data.members.map((addr) => (
              <span
                className="inline-flex items-center gap-1 rounded bg-[var(--color-bg-raised)] px-2 py-0.5 text-xs font-medium text-[var(--color-text-secondary)]"
                key={addr}
              >
                {addr}
                <button
                  className="text-[var(--color-status-danger)] hover:opacity-70"
                  onClick={() => handleRemoveMember(addr)}
                >
                  x
                </button>
              </span>
            ))}
          </div>
        ) : (
          <span className="text-xs text-[var(--color-text-tertiary)]">
            No members
          </span>
        )}
      </div>
    </div>
  )
}
