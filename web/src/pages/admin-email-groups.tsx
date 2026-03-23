import type { DomainInfo } from '@/lib/types'

import { Fragment, useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson } from '@/lib/api'

type EmailGroupInfo = {
  address: string
  created_at: string
  description: string
  domain: string
  id: number
  name: string
}

export function AdminEmailGroups() {
  const [groups, setGroups] = useState<EmailGroupInfo[]>([])
  const [domains, setDomains] = useState<DomainInfo[]>([])
  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<null | number>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)
  const [form, setForm] = useState({
    address: '',
    description: '',
    domain: '',
    name: '',
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
    void loadGroups()
    void loadDomains()
  }, [loadGroups, loadDomains])

  const handleAdd = async () => {
    if (!form.address.trim() || !form.domain || !form.name.trim()) return
    try {
      await postJson('/admin/email-groups', {
        address: form.address.trim(),
        description: form.description.trim(),
        domain: form.domain,
        name: form.name.trim(),
      })
      toast.success(`Email group "${form.name.trim()}" created`)
      setForm({ address: '', description: '', domain: '', name: '' })
      setAdding(false)
      loadGroups()
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : 'Failed to create email group'
      )
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
      toast.error(
        e instanceof Error ? e.message : 'Failed to delete email group'
      )
      setDeleteTarget(null)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Email Groups</h2>
        <button
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add Email Group
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, address: e.target.value })}
              placeholder="team@example.com"
              value={form.address}
            />
            <select
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              value={form.domain}
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
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Group name"
              value={form.name}
            />
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) =>
                setForm({ ...form, description: e.target.value })
              }
              placeholder="Description"
              value={form.description}
            />
          </div>
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
                      className="mr-3 text-xs text-[var(--color-brand-primary)] hover:opacity-80"
                      onClick={() =>
                        setExpandedId(expandedId === group.id ? null : group.id)
                      }
                    >
                      {expandedId === group.id ? 'Hide' : 'Members'}
                    </button>
                    <button
                      className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
                      onClick={() => setDeleteTarget(group.id)}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
                {expandedId === group.id && (
                  <tr>
                    <td colSpan={5}>
                      <EmailGroupMembers group={group} onChanged={loadGroups} />
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
                  No email groups configured
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
              Delete this email group? This cannot be undone.
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

function EmailGroupMembers({
  group,
  onChanged,
}: {
  group: EmailGroupInfo
  onChanged: () => void
}) {
  const [members, setMembers] = useState<null | string[]>(null)
  const [newMember, setNewMember] = useState('')

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<string[]>(
        `/admin/email-groups/${group.id}/members`
      )
      setMembers(data)
    } catch {
      // keep current state
    }
  }, [group.id])

  useEffect(() => {
    void load()
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
    if (!window.confirm(`Remove member "${address}" from this email group?`))
      return
    try {
      await deleteJson(
        `/admin/email-groups/${group.id}/members/${encodeURIComponent(address)}`
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
    <div className="space-y-4 px-4 pt-1 pb-4">
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
        {members.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {members.map((addr) => (
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
