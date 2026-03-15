import { Fragment, useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson, putJson } from '@/lib/api'
import type { DomainInfo } from '@/lib/types'

type GroupInfo = {
  id: number
  name: string
  domain: string | null
  description: string
  is_builtin: boolean
}

type ExpandedData = {
  permissions: string[]
  members: string[]
}

function GroupDetail({
  group,
  allPermissions,
  onChanged,
}: {
  group: GroupInfo
  allPermissions: string[]
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
      setData({ permissions: perms, members })
    } catch {
      // keep current state
    }
  }, [group.id])

  useEffect(() => {
    load()
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
      toast.error(e instanceof Error ? e.message : 'Failed to update permissions')
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
        `/admin/groups/${group.id}/members/${encodeURIComponent(address)}`,
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
    <div className="space-y-4 px-4 pb-4 pt-1">
      <div>
        <h4 className="mb-2 text-xs font-medium text-[var(--color-text-secondary)]">
          Permissions
        </h4>
        <div className="flex flex-wrap gap-2">
          {allPermissions.map((perm) => (
            <label
              key={perm}
              className="flex items-center gap-1.5 rounded px-2 py-1 text-xs hover:bg-[var(--color-hover)]"
            >
              <input
                type="checkbox"
                checked={data.permissions.includes(perm)}
                onChange={(e) => handleTogglePermission(perm, e.target.checked)}
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
            value={newMember}
            onChange={(e) => setNewMember(e.target.value)}
            placeholder="user@example.com"
            className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            onKeyDown={(e) => e.key === 'Enter' && handleAddMember()}
          />
          <button
            onClick={handleAddMember}
            className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)]"
          >
            Add
          </button>
        </div>
        {data.members.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {data.members.map((addr) => (
              <span
                key={addr}
                className="inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs font-medium bg-[var(--color-bg-raised)] text-[var(--color-text-secondary)]"
              >
                {addr}
                <button
                  onClick={() => handleRemoveMember(addr)}
                  className="text-[var(--color-status-danger)] hover:opacity-70"
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

export function AdminGroups() {
  const [groups, setGroups] = useState<GroupInfo[]>([])
  const [domains, setDomains] = useState<DomainInfo[]>([])
  const [allPermissions, setAllPermissions] = useState<string[]>([])
  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<number | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<number | null>(null)
  const [form, setForm] = useState({
    name: '',
    domain: '',
    description: '',
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
    loadGroups()
    loadDomains()
    loadPermissions()
  }, [loadGroups, loadDomains, loadPermissions])

  const handleAdd = async () => {
    if (!form.name.trim()) return
    try {
      await postJson('/admin/groups', {
        name: form.name.trim(),
        domain: form.domain || undefined,
        description: form.description.trim(),
      })
      toast.success(`Group "${form.name.trim()}" added`)
      setForm({ name: '', domain: '', description: '' })
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
          onClick={() => setAdding(true)}
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
        >
          Add Group
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Group name"
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            />
            <select
              value={form.domain}
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
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
            value={form.description}
            onChange={(e) => setForm({ ...form, description: e.target.value })}
            placeholder="Description"
            className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
          />
          <div className="flex gap-2">
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
                      <span className="inline-block rounded px-2 py-0.5 text-xs font-medium bg-[var(--color-bg-raised)] text-[var(--color-text-secondary)]">
                        builtin
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {group.description}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <button
                      onClick={() =>
                        setExpandedId(expandedId === group.id ? null : group.id)
                      }
                      className="mr-3 text-xs text-[var(--color-brand-primary)] hover:opacity-80"
                    >
                      {expandedId === group.id ? 'Hide' : 'Manage'}
                    </button>
                    {!group.is_builtin && (
                      <button
                        onClick={() => setDeleteTarget(group.id)}
                        className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
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
                        group={group}
                        allPermissions={allPermissions}
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
                  colSpan={5}
                  className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
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
          <div className="w-full max-w-sm rounded-lg bg-[var(--color-bg-raised)] p-6" style={{ boxShadow: 'var(--shadow-lg)' }}>
            <p className="mb-4 text-sm text-[var(--color-text-secondary)]">Delete this group? This cannot be undone.</p>
            <div className="flex justify-end gap-2">
              <button onClick={() => setDeleteTarget(null)} className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]">
                Cancel
              </button>
              <button onClick={() => handleDelete(deleteTarget)} className="rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90">
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
