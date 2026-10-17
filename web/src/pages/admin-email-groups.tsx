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
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet, adminPost } from '@/wire/endpoints/admin'

const HEADERS = ['Address', 'Domain', 'Name', 'Description', '']

const EMPTY_FORM = { address: '', description: '', domain: '', name: '' }

export function AdminEmailGroups() {
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
    successMsg: (vars) => `Email group "${vars.name.trim()}" added`,
  })

  const deleteGroup = useAdminMutation({
    invalidateKey: adminKeys.emailGroups(),
    successMsg: 'Email group deleted',
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
      header: 'Address',
      key: 'address',
      cell: (g) => (
        <Copyable value={g.address}>
          <span className="font-medium">{g.address}</span>
        </Copyable>
      ),
    },
    {
      header: 'Domain',
      key: 'domain',
      cell: (g) => <span className="text-fg-secondary">{g.domain}</span>,
    },
    {
      header: 'Name',
      key: 'name',
      cell: (g) => <span className="text-fg-secondary">{g.name}</span>,
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
            {membersLabel(expandedId === g.id)}
          </button>
          <button
            className="text-danger hover:opacity-70"
            onClick={() => setDeleteTarget(g)}
            type="button"
          >
            Delete
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
    if (isPending) return <AdminTableSkeleton cols={5} headers={HEADERS} rows={4} />
    if (error) {
      return <AdminErrorState error={error} onRetry={() => refetch()} what="the email group list" />
    }
    if (groups.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description="An email group is one address that delivers each message to every member, inside or outside this server. Add a group, then add its members"
          icon={<Mailbox className="h-10 w-10" />}
          title="No email groups yet"
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        detail={membersOf}
        label="Email groups"
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
            Add email group
          </button>
        )
      }
      title="Email groups"
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
          confirmLabel={pendingLabel(deleteGroup.isPending, 'Deleting...', 'Delete email group')}
          consequence={`Mail sent to ${deleteTarget.address} stops reaching its members, and the member list is deleted with the group. Mail the members already received is kept`}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget.id)}
          pending={deleteGroup.isPending}
          title={`Delete ${deleteTarget.address}?`}
        />
      )}
    </AdminPageShell>
  )
}

function membersLabel(open: boolean): string {
  if (open) return 'Hide'
  return 'Members'
}

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}
