import type { AccountInfo } from '@/lib/types'

import { useState } from 'react'

import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminPut } from '@/wire/endpoints/admin'

export function DisplayNameCell({ account }: { account: AccountInfo }) {
  const t = useT()
  const [editing, setEditing] = useState(false)
  const [value, setValue] = useState(account.display_name)

  const updateName = useAdminMutation({
    invalidateKey: adminKeys.accounts(),
    mutationFn: (displayName: string) =>
      adminPut(`/admin/accounts/${encodeURIComponent(account.address)}`, {
        display_name: displayName,
      }),
    successMsg: () => t('admin.accounts.nameUpdated', { address: account.address }),
  })

  const handleSave = () => {
    const trimmed = value.trim()
    if (trimmed === account.display_name) {
      setEditing(false)
      return
    }
    updateName.mutate(trimmed, { onSuccess: () => setEditing(false) })
  }

  if (!editing) {
    return (
      <button
        className="text-fg-secondary w-full cursor-pointer text-left hover:underline"
        onClick={() => {
          setValue(account.display_name)
          setEditing(true)
        }}
        title={t('admin.clickToEdit')}
      >
        {account.display_name || '—'}
      </button>
    )
  }

  return (
    <div className="flex items-center gap-1.5">
      <input
        aria-label={t('admin.displayName')}
        autoFocus
        className="border-border bg-bg-secondary w-32 rounded-md border px-2 py-0.5 text-sm"
        disabled={updateName.isPending}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') handleSave()
          if (e.key === 'Escape') setEditing(false)
        }}
        value={value}
      />
      <button
        className="text-success text-xs hover:opacity-80 disabled:opacity-50"
        disabled={updateName.isPending}
        onClick={handleSave}
      >
        {t(saveLabel(updateName.isPending))}
      </button>
      <button
        className="text-fg-muted hover:text-fg-secondary text-xs transition-colors"
        disabled={updateName.isPending}
        onClick={() => setEditing(false)}
      >
        {t('common.cancel')}
      </button>
    </div>
  )
}

function saveLabel(pending: boolean): Key {
  if (pending) return 'admin.busyDots'
  return 'admin.save'
}
