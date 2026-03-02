import { useAtom } from 'jotai'
import { useCallback, useEffect, useState } from 'react'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { AccountInfo } from '@/lib/types'
import { accountsAtom } from '@/store/admin'

export function AdminAccounts() {
  const [accounts, setAccounts] = useAtom(accountsAtom)
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({
    address: '',
    domain: '',
    displayName: '',
    password: '',
  })

  const loadAccounts = useCallback(async () => {
    try {
      const data = await fetchJson<AccountInfo[]>('/admin/accounts')
      setAccounts(data)
    } catch {
      // keep current state on error
    }
  }, [setAccounts])

  useEffect(() => {
    loadAccounts()
  }, [loadAccounts])

  const handleAdd = async () => {
    if (!form.address.trim() || !form.domain.trim()) return
    await postJson('/admin/accounts', {
      address: form.address.trim(),
      domain: form.domain.trim(),
      display_name: form.displayName.trim(),
      password: form.password,
    })
    setForm({ address: '', domain: '', displayName: '', password: '' })
    setAdding(false)
    loadAccounts()
  }

  const handleDelete = async (address: string) => {
    await deleteJson(`/admin/accounts/${encodeURIComponent(address)}`)
    loadAccounts()
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Accounts</h2>
        <button
          onClick={() => setAdding(true)}
          className="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
        >
          Add Account
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <div className="flex gap-2">
            <input
              value={form.address}
              onChange={(e) => setForm({ ...form, address: e.target.value })}
              placeholder="user@example.com"
              className="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
            />
            <input
              value={form.domain}
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              placeholder="example.com"
              className="w-40 rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
            />
          </div>
          <div className="flex gap-2">
            <input
              value={form.displayName}
              onChange={(e) =>
                setForm({ ...form, displayName: e.target.value })
              }
              placeholder="Display Name"
              className="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
            />
            <input
              type="password"
              value={form.password}
              onChange={(e) => setForm({ ...form, password: e.target.value })}
              placeholder="Password"
              className="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
            />
          </div>
          <div className="flex gap-2">
            <button
              onClick={handleAdd}
              className="rounded-md bg-zinc-900 px-3 py-1.5 text-sm text-white dark:bg-zinc-100 dark:text-zinc-900"
            >
              Save
            </button>
            <button
              onClick={() => setAdding(false)}
              className="rounded-md px-3 py-1.5 text-sm text-zinc-500"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-800">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-zinc-200 bg-zinc-50 dark:border-zinc-800 dark:bg-zinc-900">
            <tr>
              <th className="px-4 py-2.5 font-medium">Address</th>
              <th className="px-4 py-2.5 font-medium">Display Name</th>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Status</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {accounts.map((account) => (
              <tr
                key={account.address}
                className="border-b border-zinc-100 last:border-0 dark:border-zinc-800/50"
              >
                <td className="px-4 py-3 font-medium">{account.address}</td>
                <td className="px-4 py-3 text-zinc-500">
                  {account.display_name}
                </td>
                <td className="px-4 py-3 text-zinc-500">{account.domain}</td>
                <td className="px-4 py-3">
                  {account.active ? (
                    <span className="inline-block h-2 w-2 rounded-full bg-green-500" />
                  ) : (
                    <span className="inline-block h-2 w-2 rounded-full bg-zinc-300 dark:bg-zinc-600" />
                  )}
                </td>
                <td className="px-4 py-3 text-right">
                  <button
                    onClick={() => handleDelete(account.address)}
                    className="text-xs text-red-500 hover:text-red-700"
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
            {accounts.length === 0 && (
              <tr>
                <td colSpan={5} className="px-4 py-8 text-center text-zinc-400">
                  No accounts configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}
