import type { DomainInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Mailbox } from 'lucide-react'
import { Fragment, useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { MobileModal } from '@/components/mobile-modal'
import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { deleteJson, fetchList, postJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

type EmailGroupInfo = {
  address: string
  created_at: string
  description: string
  domain: string
  id: number
  name: string
}

const HEADERS = ['Address', 'Domain', 'Name', 'Description', 'Actions']

export function AdminEmailGroups() {
  const {
    data: groupsData,
    error,
    isPending,
    refetch,
  } = useQuery({
    queryKey: adminKeys.emailGroups(),
    queryFn: ({ signal }) => fetchList<EmailGroupInfo>('/admin/email-groups', signal),
  })
  const groups = groupsData ?? []

  const { data: domainsData } = useQuery({
    queryKey: adminKeys.domains(),
    queryFn: ({ signal }) => fetchList<DomainInfo>('/admin/domains', signal),
  })
  const domains = domainsData ?? []

  const [adding, setAdding] = useState(false)
  const [expandedId, setExpandedId] = useState<null | number>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)
  const [form, setForm] = useState({
    address: '',
    description: '',
    domain: '',
    name: '',
  })

  const addGroup = useAdminMutation({
    invalidateKey: adminKeys.emailGroups(),
    mutationFn: (vars: typeof form) =>
      postJson('/admin/email-groups', {
        address: vars.address.trim(),
        description: vars.description.trim(),
        domain: vars.domain,
        name: vars.name.trim(),
      }),
    successMsg: (vars) => `Email group "${vars.name.trim()}" created`,
  })

  const deleteGroup = useAdminMutation({
    invalidateKey: adminKeys.emailGroups(),
    successMsg: 'Email group deleted',
    mutationFn: (id: number) => deleteJson(`/admin/email-groups/${id}`),
  })

  const formValid = !!form.address.trim() && !!form.domain && !!form.name.trim()

  const handleAdd = () => {
    if (!formValid) return
    addGroup.mutate(form, {
      onSuccess: () => {
        setForm({ address: '', description: '', domain: '', name: '' })
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

  return (
    <AdminPageShell
      actions={
        !adding && (
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
            onClick={() => setAdding(true)}
          >
            Add Email Group
          </button>
        )
      }
      title="Email Groups"
    >
      {adding && (
        <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
          <div className="flex gap-2">
            <input
              aria-label="Email address"
              autoFocus
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, address: e.target.value })}
              placeholder="team@example.com"
              value={form.address}
            />
            <select
              aria-label="Domain"
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
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
              aria-label="Group name"
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Group name"
              value={form.name}
            />
            <input
              aria-label="Description"
              className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, description: e.target.value })}
              placeholder="Description"
              value={form.description}
            />
          </div>
          <div className="flex gap-2">
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
              disabled={!formValid || addGroup.isPending}
              onClick={handleAdd}
            >
              {addGroup.isPending ? 'Saving...' : 'Save'}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm({ address: '', description: '', domain: '', name: '' })
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
          description="Email groups are addresses that fan out to multiple recipients."
          icon={<Mailbox className="h-10 w-10" />}
          title="No email groups configured"
        />
      ) : (
        <ScrollableTable>
          <table className="w-full text-left text-sm">
            <thead className="border-border bg-bg-secondary border-b">
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
                  <tr className="border-border border-b last:border-0">
                    <td className="px-4 py-3 font-medium">{group.address}</td>
                    <td className="text-fg-secondary px-4 py-3">{group.domain}</td>
                    <td className="text-fg-secondary px-4 py-3">{group.name}</td>
                    <td className="text-fg-secondary px-4 py-3">{group.description}</td>
                    <td className="px-4 py-3 text-right">
                      <button
                        className="text-accent mr-3 text-xs hover:opacity-80"
                        onClick={() => setExpandedId(expandedId === group.id ? null : group.id)}
                      >
                        {expandedId === group.id ? 'Hide' : 'Members'}
                      </button>
                      <button
                        className="text-danger text-xs transition-colors hover:opacity-70"
                        onClick={() => setDeleteTarget(group.id)}
                      >
                        Delete
                      </button>
                    </td>
                  </tr>
                  {expandedId === group.id && (
                    <tr>
                      <td colSpan={5}>
                        <EmailGroupMembers group={group} />
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
              Delete this email group? This cannot be undone.
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

function EmailGroupMembers({ group }: { group: EmailGroupInfo }) {
  const [newMember, setNewMember] = useState('')

  const { data: membersData } = useQuery({
    queryKey: adminKeys.emailGroupMembers(group.id),
    queryFn: ({ signal }) => fetchList<string>(`/admin/email-groups/${group.id}/members`, signal),
  })
  const members = membersData ?? null

  const addMember = useAdminMutation({
    invalidateKey: adminKeys.emailGroupMembers(group.id),
    mutationFn: (address: string) =>
      postJson(`/admin/email-groups/${group.id}/members`, { address }),
    successMsg: (address) => `Member "${address}" added`,
  })

  const removeMember = useAdminMutation({
    invalidateKey: adminKeys.emailGroupMembers(group.id),
    mutationFn: (address: string) =>
      deleteJson(`/admin/email-groups/${group.id}/members/${encodeURIComponent(address)}`),
    successMsg: (address) => `Member "${address}" removed`,
  })

  const handleAddMember = () => {
    const address = newMember.trim()
    if (!address) return
    addMember.mutate(address, { onSuccess: () => setNewMember('') })
  }

  const handleRemoveMember = (address: string) => {
    if (!window.confirm(`Remove member "${address}" from this email group?`)) return
    removeMember.mutate(address)
  }

  if (!members) {
    return <div className="text-fg-muted px-4 py-3 text-sm">Loading...</div>
  }

  return (
    <div className="space-y-4 px-4 pt-1 pb-4">
      <div>
        <h4 className="text-fg-secondary mb-2 text-xs font-medium">Members</h4>
        <div className="mb-2 flex gap-2">
          <input
            aria-label="New member address"
            className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
            onChange={(e) => setNewMember(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleAddMember()}
            placeholder="user@example.com"
            value={newMember}
          />
          <button
            className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
            disabled={!newMember.trim() || addMember.isPending}
            onClick={handleAddMember}
          >
            {addMember.isPending ? 'Adding...' : 'Add'}
          </button>
        </div>
        {members.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {members.map((addr) => (
              <span
                className="bg-surface text-fg-secondary inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs font-medium"
                key={addr}
              >
                {addr}
                <button
                  aria-label={`Remove ${addr}`}
                  className="text-danger hover:opacity-70 disabled:opacity-50"
                  disabled={removeMember.isPending}
                  onClick={() => handleRemoveMember(addr)}
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        ) : (
          <span className="text-fg-muted text-xs">No members</span>
        )}
      </div>
    </div>
  )
}
