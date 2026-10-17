import { useState } from 'react'

import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminPost } from '@/wire/endpoints/admin'

const EMPTY_FORM = { address: '', displayName: '', domain: '', password: '' }

export function AddAccountForm({ onDone }: { onDone: () => void }) {
  const [form, setForm] = useState(EMPTY_FORM)

  const addAccount = useAdminMutation({
    invalidateKey: adminKeys.accounts(),
    // no `domain`: fastcore derives it from the address
    mutationFn: (vars: { address: string; displayName: string; password: string }) =>
      adminPost<{ message?: string; success: boolean }>('/admin/accounts', {
        address: vars.address,
        display_name: vars.displayName,
        password: vars.password,
      }),
    successMsg: (vars) => `Account "${vars.address}" added`,
  })

  const handleAdd = () => {
    const local = form.address.trim().replace(/@.*$/, '')
    const domain = form.domain.trim()
    if (!local || !domain || !form.password) return
    addAccount.mutate(
      {
        address: `${local}@${domain}`,
        displayName: form.displayName.trim(),
        password: form.password,
      },
      {
        onSuccess: () => {
          setForm(EMPTY_FORM)
          onDone()
        },
      }
    )
  }

  const incomplete = !form.address.trim() || !form.domain.trim() || !form.password

  return (
    <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
      <div className="flex gap-2">
        <input
          aria-label="Username"
          autoFocus
          className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, address: e.target.value })}
          placeholder="username"
          value={form.address}
        />
        <input
          aria-label="Domain"
          className="border-border bg-bg-secondary w-40 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, domain: e.target.value })}
          placeholder="example.com"
          value={form.domain}
        />
      </div>
      <div className="flex gap-2">
        <input
          aria-label="Display name"
          className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, displayName: e.target.value })}
          placeholder="Display name"
          value={form.displayName}
        />
        <input
          aria-label="Password"
          autoComplete="new-password"
          className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, password: e.target.value })}
          placeholder="Password"
          type="password"
          value={form.password}
        />
      </div>
      <div className="flex gap-2">
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
          disabled={incomplete || addAccount.isPending}
          onClick={handleAdd}
        >
          {saveLabel(addAccount.isPending)}
        </button>
        <button
          className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
          onClick={() => {
            setForm(EMPTY_FORM)
            onDone()
          }}
        >
          Cancel
        </button>
      </div>
    </div>
  )
}

function saveLabel(pending: boolean): string {
  if (pending) return 'Saving...'
  return 'Save'
}
