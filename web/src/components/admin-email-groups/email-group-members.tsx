import type { EmailGroupInfo } from './email-group-info'

import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { ConfirmAction } from '@/components/admin-page'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminPost, adminStringListGet } from '@/wire/endpoints/admin'

export function EmailGroupMembers({ group }: { group: EmailGroupInfo }) {
  const [newMember, setNewMember] = useState('')
  const [removeTarget, setRemoveTarget] = useState<null | string>(null)

  const { data } = useQuery({
    queryKey: adminKeys.emailGroupMembers(group.id),
    queryFn: ({ signal }) => adminStringListGet(`/admin/email-groups/${group.id}/members`, signal),
  })

  const addMember = useAdminMutation({
    invalidateKey: adminKeys.emailGroupMembers(group.id),
    mutationFn: (address: string) =>
      adminPost(`/admin/email-groups/${group.id}/members`, { address }),
    successMsg: (address) => `Member "${address}" added`,
  })

  const removeMember = useAdminMutation({
    invalidateKey: adminKeys.emailGroupMembers(group.id),
    mutationFn: (address: string) =>
      adminDelete(`/admin/email-groups/${group.id}/members/${encodeURIComponent(address)}`),
    successMsg: (address) => `Member "${address}" removed`,
  })

  const handleAddMember = () => {
    const address = newMember.trim()
    if (!address) return
    addMember.mutate(address, { onSuccess: () => setNewMember('') })
  }

  const handleRemove = (address: string) => {
    removeMember.mutate(address, { onSettled: () => setRemoveTarget(null) })
  }

  if (!data) {
    return <div className="text-fg-muted text-sm">Loading...</div>
  }

  return (
    <div>
      <h4 className="text-fg-secondary mb-2 text-xs font-medium">Members</h4>
      <div className="mb-2 flex gap-2">
        <input
          aria-label="New member address"
          className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setNewMember(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleAddMember()
          }}
          placeholder="user@example.com"
          value={newMember}
        />
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
          disabled={!newMember.trim() || addMember.isPending}
          onClick={handleAddMember}
        >
          {pendingLabel(addMember.isPending, 'Adding...', 'Add member')}
        </button>
      </div>
      <MemberList address={group.address} members={data} onRemove={setRemoveTarget} />
      {removeTarget && (
        <ConfirmAction
          confirmLabel={pendingLabel(removeMember.isPending, 'Removing...', 'Remove member')}
          consequence={`${removeTarget} stops receiving mail sent to ${group.address}. Mail already delivered to it is kept`}
          onCancel={() => setRemoveTarget(null)}
          onConfirm={() => handleRemove(removeTarget)}
          pending={removeMember.isPending}
          title={`Remove ${removeTarget} from ${group.name}?`}
        />
      )}
    </div>
  )
}

function MemberList({
  address,
  members,
  onRemove,
}: {
  address: string
  members: string[]
  onRemove: (address: string) => void
}) {
  if (members.length === 0) {
    return (
      <span className="text-fg-muted text-xs">
        No members yet. Add one so mail sent to {address} reaches someone
      </span>
    )
  }
  return (
    <ul className="divide-border divide-y text-sm">
      {members.map((addr) => (
        <li className="flex items-center justify-between py-1.5" key={addr}>
          <span className="text-fg-secondary">{addr}</span>
          <button
            className="text-danger text-xs hover:opacity-70"
            onClick={() => onRemove(addr)}
            type="button"
          >
            Remove
          </button>
        </li>
      ))}
    </ul>
  )
}

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}
