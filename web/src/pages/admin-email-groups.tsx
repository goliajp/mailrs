import { Fragment, useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { DomainInfo } from '@/lib/types'

type EmailGroupInfo = {
  id: number
  address: string
  domain: string
  name: string
  description: string
  created_at: string
}

function EmailGroupMembers({
  group,
  onChanged,
}: {
  group: EmailGroupInfo
  onChanged: () => void
}) {
  const [members, setMembers] = useState<string[] | null>(null)
  const [newMember, setNewMember] = useState('')

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<string[]>(
        `/admin/email-groups/${group.id}/members`,
      )
      setMembers(data)
    } catch {
      // keep current state
    }
  }, [group.id])

  useEffect(() => {
    load()
  }, [load])

  const handleAddMember = async () => {
    if (!newMember.trim()) return
    try {
      await postJson(`/admin/email-groups/${group.id}/members`, {
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
    if (!window.confirm(`Remove member "${address}" from this email group?`)) return
    try {
      await deleteJson(
        `/admin/email-groups/${group.id}/members/${encodeURIComponent(address)}`,
      )
      toast.success(`Member "${address}" removed`)
      load()
      onChanged()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove member')
    }
  }

  if (!members) {
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
        {members.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {members.map((addr) => (
              <span
                key={addr}
                className="inline-flex items-center gap-1 rounded bg-[var(--color-bg-raised)] px-2 py-0.5 text-xs font-medium text-[var(--color-text-secondary)]"
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

export function AdminEmailGroups() {
  const [groups, setGroups] = useState<EmailGroupInfo[]>([])
  const [domains, setDomains] = useState<DomainInfo[]>([])
  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<number | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<number | null>(null)
  const [form, setForm] = useState({
    address: '',
    domain: '',
    name: '',
    description: '',
  })

  const loadGroups = useCallback(async () => {
    try {
      const data = await fetchJson<EmailGroupInfo[]>('/admin/email-groups')
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

  useEffect(() => {
    loadGroups()
    loadDomains()
  }, [loadGroups, loadDomains])

  const handleAdd = async () => {
    if (!form.address.trim() || !form.domain || !form.name.trim()) return
    try {
      await postJson('/admin/email-groups', {
        address: form.address.trim(),
        domain: form.domain,
        name: form.name.trim(),
        description: form.description.trim(),
      })
      toast.success(`Email group "${form.name.trim()}" created`)
      setForm({ address: '', domain: '', name: '', description: '' })
      setAdding(false)
      loadGroups()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create email group')
    }
  }

  const handleDelete = async (id: number) => {
    try {
      await deleteJson(`/admin/email-groups/${id}`)
      toast.success('Email group deleted')
      setDeleteTarget(null)
      if (expandedId === id) setExpandedId(null)
      loadGroups()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete email group')
      setDeleteTarget(null)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Email Groups</h2>
        <button
          onClick={() => setAdding(true)}
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
        >
          Add Email Group
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              value={form.address}
              onChange={(e) => setForm({ ...form, address: e.target.value })}
              placeholder="team@example.com"
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            />
            <select
              value={form.domain}
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            >
              <option value="">Select domain...</option>
              {domains.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                </option>
              ))}
            </select>
          </div>
          <div className="flex gap-2">
            <input
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Group name"
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            />
            <input
              value={form.description}
              onChange={(e) => setForm({ ...form, description: e.target.value })}
              placeholder="Description"
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
            />
          </div>
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
              <th className="px-4 py-2.5 font-medium">Address</th>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Name</th>
              <th className="px-4 py-2.5 font-medium">Description</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {groups.map((group) => (
              <Fragment key={group.id}>
                <tr className="border-b border-[var(--color-border-default)] last:border-0">
                  <td className="px-4 py-3 font-medium">{group.address}</td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {group.domain}
                  </td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {group.name}
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
                      {expandedId === group.id ? 'Hide' : 'Members'}
                    </button>
                    <button
                      onClick={() => setDeleteTarget(group.id)}
                      className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
                    >
                      Delete
                    </button>
                  </td>
                </tr>
                {expandedId === group.id && (
                  <tr>
                    <td colSpan={5}>
                      <EmailGroupMembers
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
                  colSpan={5}
                  className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
                >
                  No email groups configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {deleteTarget !== null && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-sm rounded-lg bg-[var(--color-bg-raised)] p-6" style={{ boxShadow: 'var(--shadow-lg)' }}>
            <p className="mb-4 text-sm text-[var(--color-text-secondary)]">Delete this email group? This cannot be undone.</p>
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
