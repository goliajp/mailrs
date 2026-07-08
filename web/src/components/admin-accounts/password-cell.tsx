import type { AccountInfo } from '@/lib/types'

import { useState } from 'react'

import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminPost } from '@/wire/endpoints/admin'

export function PasswordCell({ account }: { account: AccountInfo }) {
  const [editing, setEditing] = useState(false)
  const [password, setPassword] = useState('')

  const updatePassword = useAdminMutation({
    invalidateKey: adminKeys.accounts(),
    mutationFn: (newPassword: string) =>
      adminPost('/admin/accounts', {
        address: account.address,
        display_name: account.display_name,
        domain: account.domain,
        password: newPassword,
      }),
    successMsg: () => `Password updated for "${account.address}"`,
  })

  const handleSave = () => {
    const trimmed = password.trim()
    if (!trimmed) return
    updatePassword.mutate(trimmed, {
      onSuccess: () => {
        setPassword('')
        setEditing(false)
      },
    })
  }

  if (!editing) {
    return (
      <button className="text-accent text-xs hover:opacity-80" onClick={() => setEditing(true)}>
        Change
      </button>
    )
  }

  return (
    <div className="flex items-center gap-1.5">
      <input
        aria-label="New password"
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
        placeholder="New password"
        type="password"
        value={password}
      />
      <button
        className="text-success text-xs hover:opacity-80 disabled:opacity-50"
        disabled={updatePassword.isPending || !password.trim()}
        onClick={handleSave}
      >
        {updatePassword.isPending ? '...' : 'Save'}
      </button>
      <button
        className="text-fg-muted hover:text-fg-secondary text-xs transition-colors"
        disabled={updatePassword.isPending}
        onClick={() => {
          setPassword('')
          setEditing(false)
        }}
      >
        Cancel
      </button>
    </div>
  )
}
