import type { GroupInfo } from './group-info'

import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { ConfirmAction } from '@/components/admin-page'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminPost, adminPut, adminStringListGet } from '@/wire/endpoints/admin'

export function GroupDetail({
  allPermissions,
  group,
}: {
  allPermissions: string[]
  group: GroupInfo
}) {
  const [newMember, setNewMember] = useState('')
  const [removeTarget, setRemoveTarget] = useState<null | string>(null)

  const { data: permissionsData } = useQuery({
    queryKey: adminKeys.groupPermissions(group.id),
    queryFn: ({ signal }) => adminStringListGet(`/admin/groups/${group.id}/permissions`, signal),
  })
  const permissions = permissionsData ?? []

  const { data: membersData } = useQuery({
    queryKey: adminKeys.groupMembers(group.id),
    queryFn: ({ signal }) => adminStringListGet(`/admin/groups/${group.id}/members`, signal),
  })
  const members = membersData ?? []

  const updatePermissions = useAdminMutation({
    invalidateKey: adminKeys.groupPermissions(group.id),
    successMsg: 'Permissions updated',
    mutationFn: (perms: string[]) =>
      adminPut(`/admin/groups/${group.id}/permissions`, { permissions: perms }),
  })

  const addMember = useAdminMutation({
    invalidateKey: adminKeys.groupMembers(group.id),
    mutationFn: (address: string) => adminPost(`/admin/groups/${group.id}/members`, { address }),
    successMsg: (address) => `Member "${address}" added`,
  })

  const removeMember = useAdminMutation({
    invalidateKey: adminKeys.groupMembers(group.id),
    mutationFn: (address: string) =>
      adminDelete(`/admin/groups/${group.id}/members/${encodeURIComponent(address)}`),
    successMsg: (address) => `Member "${address}" removed`,
  })

  const loading = !permissionsData || !membersData

  const handleTogglePermission = (perm: string, checked: boolean) => {
    updatePermissions.mutate(toggled(permissions, perm, checked))
  }

  const handleAddMember = () => {
    const address = newMember.trim()
    if (!address) return
    addMember.mutate(address, {
      onSuccess: () => {
        setNewMember('')
      },
    })
  }

  const handleRemove = (address: string) => {
    removeMember.mutate(address, { onSettled: () => setRemoveTarget(null) })
  }

  if (loading) {
    return <div className="text-fg-muted text-sm">Loading...</div>
  }

  return (
    <div className="space-y-4">
      <div>
        <h4 className="text-fg-secondary mb-2 text-xs font-medium">Permissions</h4>
        <div className="flex flex-wrap gap-2">
          {allPermissions.map((perm) => (
            <label
              className="hover:bg-bg-secondary flex items-center gap-1.5 rounded px-2 py-1 text-xs"
              key={perm}
            >
              <input
                checked={permissions.includes(perm)}
                disabled={updatePermissions.isPending}
                onChange={(e) => handleTogglePermission(perm, e.target.checked)}
                type="checkbox"
              />
              {perm}
            </label>
          ))}
          {allPermissions.length === 0 && (
            <span className="text-fg-muted text-xs">The server lists no permissions to grant</span>
          )}
        </div>
      </div>

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
        <MemberList members={members} onRemove={setRemoveTarget} />
      </div>

      {removeTarget && (
        <ConfirmAction
          confirmLabel={pendingLabel(removeMember.isPending, 'Removing...', 'Remove member')}
          consequence={`${removeTarget} loses the permissions ${group.name} grants, unless its own account or another group grants them too. The account is kept`}
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
  members,
  onRemove,
}: {
  members: string[]
  onRemove: (address: string) => void
}) {
  if (members.length === 0) {
    return (
      <span className="text-fg-muted text-xs">
        No members yet. An account added here gets every permission checked above
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

function toggled(current: string[], perm: string, on: boolean): string[] {
  if (on) return [...current, perm]
  return current.filter((p) => p !== perm)
}
