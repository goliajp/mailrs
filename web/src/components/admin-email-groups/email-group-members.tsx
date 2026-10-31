import type { EmailGroupInfo } from './email-group-info'

import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { ConfirmAction } from '@/components/admin-page'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminPost, adminStringListGet } from '@/wire/endpoints/admin'

export function EmailGroupMembers({ group }: { group: EmailGroupInfo }) {
  const t = useT()
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
    successMsg: (address) => t('admin.memberAdded', { address }),
  })

  const removeMember = useAdminMutation({
    invalidateKey: adminKeys.emailGroupMembers(group.id),
    mutationFn: (address: string) =>
      adminDelete(`/admin/email-groups/${group.id}/members/${encodeURIComponent(address)}`),
    successMsg: (address) => t('admin.memberRemoved', { address }),
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
    return <div className="text-fg-muted text-sm">{t('admin.loading')}</div>
  }

  return (
    <div>
      <h4 className="text-fg-secondary mb-2 text-xs font-medium">{t('admin.members')}</h4>
      <div className="mb-2 flex gap-2">
        <input
          aria-label={t('admin.newMemberAddress')}
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
          {t(pendingLabel(addMember.isPending, 'admin.adding', 'admin.addMember'))}
        </button>
      </div>
      <MemberList address={group.address} members={data} onRemove={setRemoveTarget} />
      {removeTarget && (
        <ConfirmAction
          confirmLabel={t(
            pendingLabel(removeMember.isPending, 'admin.removing', 'admin.removeMember')
          )}
          consequence={t('admin.emailGroups.removeConsequence', {
            address: group.address,
            member: removeTarget,
          })}
          onCancel={() => setRemoveTarget(null)}
          onConfirm={() => handleRemove(removeTarget)}
          pending={removeMember.isPending}
          title={t('admin.removeFromTitle', { group: group.name, member: removeTarget })}
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
  const t = useT()
  if (members.length === 0) {
    return (
      <span className="text-fg-muted text-xs">{t('admin.emailGroups.noMembers', { address })}</span>
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
            {t('admin.remove')}
          </button>
        </li>
      ))}
    </ul>
  )
}

function pendingLabel(pending: boolean, busy: Key, idle: Key): Key {
  if (pending) return busy
  return idle
}
