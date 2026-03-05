import { useAtom } from 'jotai'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { AccountInfo, QuotaInfo } from '@/lib/types'
import { accountsAtom } from '@/store/admin'

const PAGE_SIZE = 20

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

type QuotaState =
  | { status: 'loading' }
  | { status: 'none' }
  | { status: 'loaded'; quotaBytes: number }

function QuotaCell({ address }: { address: string }) {
  const [quota, setQuota] = useState<QuotaState>({ status: 'loading' })

  useEffect(() => {
    let cancelled = false
    fetchJson<QuotaInfo>(
      `/admin/accounts/${encodeURIComponent(address)}/quota`,
    )
      .then((data) => {
        if (!cancelled) {
          setQuota({ status: 'loaded', quotaBytes: data.quota_bytes })
        }
      })
      .catch(() => {
        if (!cancelled) {
          setQuota({ status: 'none' })
        }
      })
    return () => {
      cancelled = true
    }
  }, [address])

  if (quota.status === 'loading') {
    return (
      <span className="text-xs text-zinc-400">Loading...</span>
    )
  }

  if (quota.status === 'none') {
    return (
      <span className="text-xs text-zinc-400">No quota set</span>
    )
  }

  const totalBytes = quota.quotaBytes
  const formatted = formatBytes(totalBytes)

  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 w-20 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-700">
        <div
          className="h-full rounded-full bg-blue-500"
          style={{ width: '0%' }}
        />
      </div>
      <span className="text-xs text-zinc-500">{formatted}</span>
    </div>
  )
}

function PasswordCell({
  account,
  onSaved,
}: {
  account: AccountInfo
  onSaved: () => void
}) {
  const [editing, setEditing] = useState(false)
  const [password, setPassword] = useState('')
  const [saving, setSaving] = useState(false)

  const handleSave = async () => {
    if (!password.trim()) return
    setSaving(true)
    try {
      await postJson('/admin/accounts', {
        address: account.address,
        domain: account.domain,
        display_name: account.display_name,
        password: password.trim(),
      })
      toast.success(`Password updated for "${account.address}"`)
      setPassword('')
      setEditing(false)
      onSaved()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update password')
    } finally {
      setSaving(false)
    }
  }

  if (!editing) {
    return (
      <button
        onClick={() => setEditing(true)}
        className="text-xs text-blue-600 hover:text-blue-800 dark:text-blue-400 dark:hover:text-blue-300"
      >
        Change
      </button>
    )
  }

  return (
    <div className="flex items-center gap-1.5">
      <input
        type="password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        placeholder="New password"
        className="w-28 rounded border border-zinc-300 px-2 py-0.5 text-xs dark:border-zinc-700 dark:bg-zinc-900"
        onKeyDown={(e) => {
          if (e.key === 'Enter') handleSave()
          if (e.key === 'Escape') {
            setPassword('')
            setEditing(false)
          }
        }}
        autoFocus
        disabled={saving}
      />
      <button
        onClick={handleSave}
        disabled={saving || !password.trim()}
        className="text-xs text-green-600 hover:text-green-800 disabled:opacity-50 dark:text-green-400 dark:hover:text-green-300"
      >
        {saving ? '...' : 'Save'}
      </button>
      <button
        onClick={() => {
          setPassword('')
          setEditing(false)
        }}
        disabled={saving}
        className="text-xs text-zinc-400 hover:text-zinc-600"
      >
        Cancel
      </button>
    </div>
  )
}

function DeleteConfirmDialog({
  address,
  onConfirm,
  onCancel,
}: {
  address: string
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-sm rounded-lg bg-white p-6 shadow-xl dark:bg-zinc-900">
        <h3 className="mb-2 text-sm font-semibold">Confirm Deletion</h3>
        <p className="mb-4 text-sm text-zinc-500">
          Are you sure you want to delete{' '}
          <span className="font-medium text-zinc-900 dark:text-zinc-100">
            {address}
          </span>
          ? This action cannot be undone.
        </p>
        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-sm text-zinc-500 hover:text-zinc-700"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            className="rounded-md bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-700"
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  )
}

export function AdminAccounts() {
  const [accounts, setAccounts] = useAtom(accountsAtom)
  const [adding, setAdding] = useState(false)
  const [filter, setFilterRaw] = useState('')
  const [page, setPage] = useState(0)
  // wrap setFilter to also reset page
  const setFilter = useCallback((v: string) => {
    setFilterRaw(v)
    setPage(0)
  }, [])
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
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
    try {
      await postJson('/admin/accounts', {
        address: form.address.trim(),
        domain: form.domain.trim(),
        display_name: form.displayName.trim(),
        password: form.password,
      })
      toast.success(`Account "${form.address.trim()}" added`)
      setForm({ address: '', domain: '', displayName: '', password: '' })
      setAdding(false)
      loadAccounts()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to add account')
    }
  }

  const handleDelete = async (address: string) => {
    try {
      await deleteJson(`/admin/accounts/${encodeURIComponent(address)}`)
      toast.success(`Account "${address}" removed`)
      setDeleteTarget(null)
      loadAccounts()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to remove account')
      setDeleteTarget(null)
    }
  }

  const filtered = useMemo(() => {
    if (!filter) return accounts
    const q = filter.toLowerCase()
    return accounts.filter(
      (a) =>
        a.address.toLowerCase().includes(q) ||
        a.domain.toLowerCase().includes(q) ||
        a.display_name.toLowerCase().includes(q),
    )
  }, [accounts, filter])

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
  const safePage = Math.min(page, totalPages - 1)
  const paged = filtered.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE)

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between gap-3">
        <h2 className="shrink-0 text-lg font-semibold">Accounts</h2>
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter accounts..."
          className="min-w-0 flex-1 rounded-md border border-zinc-200 bg-zinc-50 px-3 py-1.5 text-sm outline-none placeholder:text-zinc-400 focus:border-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 dark:focus:border-zinc-500"
        />
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

      <div className="overflow-x-auto rounded-lg border border-zinc-200 dark:border-zinc-800">
        <table className="w-full min-w-[640px] text-left text-sm">
          <thead className="border-b border-zinc-200 bg-zinc-50 dark:border-zinc-800 dark:bg-zinc-900">
            <tr>
              <th className="px-4 py-2.5 font-medium">Address</th>
              <th className="px-4 py-2.5 font-medium">Display Name</th>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Quota</th>
              <th className="px-4 py-2.5 font-medium">Status</th>
              <th className="px-4 py-2.5 font-medium">Password</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {paged.map((account) => (
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
                  <QuotaCell address={account.address} />
                </td>
                <td className="px-4 py-3">
                  {account.active ? (
                    <span className="inline-block h-2 w-2 rounded-full bg-green-500" />
                  ) : (
                    <span className="inline-block h-2 w-2 rounded-full bg-zinc-300 dark:bg-zinc-600" />
                  )}
                </td>
                <td className="px-4 py-3">
                  <PasswordCell
                    account={account}
                    onSaved={loadAccounts}
                  />
                </td>
                <td className="px-4 py-3 text-right">
                  <button
                    onClick={() => setDeleteTarget(account.address)}
                    className="text-xs text-red-500 hover:text-red-700"
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
            {filtered.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-8 text-center text-zinc-400">
                  {accounts.length === 0
                    ? 'No accounts configured'
                    : 'No accounts match the filter'}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {totalPages > 1 && (
        <div className="mt-4 flex items-center justify-between text-sm text-zinc-500">
          <span>
            Showing {safePage * PAGE_SIZE + 1}--
            {Math.min((safePage + 1) * PAGE_SIZE, filtered.length)} of{' '}
            {filtered.length}
          </span>
          <div className="flex gap-1">
            <button
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              disabled={safePage === 0}
              className="rounded px-2.5 py-1 hover:bg-zinc-100 disabled:opacity-40 dark:hover:bg-zinc-800"
            >
              Prev
            </button>
            <button
              onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
              disabled={safePage >= totalPages - 1}
              className="rounded px-2.5 py-1 hover:bg-zinc-100 disabled:opacity-40 dark:hover:bg-zinc-800"
            >
              Next
            </button>
          </div>
        </div>
      )}

      {deleteTarget && (
        <DeleteConfirmDialog
          address={deleteTarget}
          onConfirm={() => handleDelete(deleteTarget)}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  )
}
