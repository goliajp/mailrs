import type { GroupInfo } from './group-info'

import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

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
    const updated = checked ? [...permissions, perm] : permissions.filter((p) => p !== perm)
    updatePermissions.mutate(updated)
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

  const handleRemoveMember = (address: string) => {
    // single-step confirm — small affordance vs full modal is intentional
    if (!window.confirm(`Remove member "${address}" from this group?`)) return
    removeMember.mutate(address)
  }

  if (loading) {
    return <div className="text-fg-muted px-4 py-3 text-sm">Loading...</div>
  }

  return (
    <div className="space-y-4 px-4 pt-1 pb-4">
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
            <span className="text-fg-muted text-xs">No permissions available</span>
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
