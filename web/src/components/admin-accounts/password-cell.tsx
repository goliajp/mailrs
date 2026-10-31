import type { AccountInfo } from '@/lib/types'

import { useState } from 'react'

import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminPost } from '@/wire/endpoints/admin'

import { CompromiseResetDialog } from './compromise-reset-dialog'

export function PasswordCell({ account }: { account: AccountInfo }) {
  const t = useT()
  const [editing, setEditing] = useState(false)
  const [password, setPassword] = useState('')
  const [compromised, setCompromised] = useState(false)

  const updatePassword = useAdminMutation({
    invalidateKey: adminKeys.accounts(),
    // its own route: re-posting the account to create one rewrote the
    // whole account, resetting quota and recovery email
    mutationFn: (newPassword: string) =>
      adminPost(`/admin/accounts/${encodeURIComponent(account.address)}/password`, {
        password: newPassword,
      }),
    successMsg: () => t('admin.accounts.passwordUpdated', { address: account.address }),
  })

  const handleSave = () => {
    if (!password) return
    updatePassword.mutate(password, {
      onSuccess: () => {
        setPassword('')
        setEditing(false)
      },
    })
  }

  if (!editing) {
    return (
      <div className="flex items-center gap-2">
        <button className="text-accent text-xs hover:opacity-80" onClick={() => setEditing(true)}>
          {t('admin.change')}
        </button>
        <button
          className="text-danger text-xs hover:opacity-80"
          onClick={() => setCompromised(true)}
          title={t('admin.accounts.compromisedTitle')}
        >
          {t('admin.accounts.compromised')}
        </button>
        {compromised && (
          <CompromiseResetDialog address={account.address} onClose={() => setCompromised(false)} />
        )}
      </div>
    )
  }

  return (
    <div className="flex items-center gap-1.5">
      <input
        aria-label={t('admin.newPassword')}
        autoComplete="new-password"
        autoFocus
        className="border-border bg-bg-secondary w-28 rounded-md border px-2 py-0.5 text-xs"
        disabled={updatePassword.isPending}
        onChange={(e) => setPassword(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') handleSave()
          if (e.key === 'Escape') {
            setPassword('')
            setEditing(false)
          }
        }}
        placeholder={t('admin.newPassword')}
        type="password"
        value={password}
      />
      <button
        className="text-success text-xs hover:opacity-80 disabled:opacity-50"
        disabled={updatePassword.isPending || !password}
        onClick={handleSave}
      >
        {t(saveLabel(updatePassword.isPending))}
      </button>
      <button
        className="text-fg-muted hover:text-fg-secondary text-xs transition-colors"
        disabled={updatePassword.isPending}
        onClick={() => {
          setPassword('')
          setEditing(false)
        }}
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
