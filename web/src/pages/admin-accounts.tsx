import type { AccountInfo, QuotaInfo } from '@/lib/types'

import { useAtom } from 'jotai'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'

import { Copyable } from '@/components/copy-button'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { accountsAtom } from '@/store/admin'

interface GroupInfo {
  description: string
  id: number
  name: string
}

interface SieveState {
  error: null | string
  script: string
  status: 'deleting' | 'idle' | 'loaded' | 'loading' | 'saving'
}

const PAGE_SIZE = 20

type QuotaState =
  | { quotaBytes: number; status: 'loaded' }
  | { status: 'loading' }
  | { status: 'none' }

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
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
  const [form, setForm] = useState({
    address: '',
    displayName: '',
    domain: '',
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
        display_name: form.displayName.trim(),
        domain: form.domain.trim(),
        password: form.password,
      })
      toast.success(`Account "${form.address.trim()}" added`)
      setForm({ address: '', displayName: '', domain: '', password: '' })
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
        a.display_name.toLowerCase().includes(q)
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
          className="min-w-0 flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)]"
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter accounts..."
          value={filter}
        />
        <button
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
          onClick={() => setAdding(true)}
        >
          Add Account
        </button>
      </div>

      {adding && (
        <div className="mb-4 space-y-2 rounded-lg border border-[var(--color-border-default)] p-4">
          <div className="flex gap-2">
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, address: e.target.value })}
              placeholder="user@example.com"
              value={form.address}
            />
            <input
              className="w-40 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, domain: e.target.value })}
              placeholder="example.com"
              value={form.domain}
            />
          </div>
          <div className="flex gap-2">
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) =>
                setForm({ ...form, displayName: e.target.value })
              }
              placeholder="Display Name"
              value={form.displayName}
            />
            <input
              className="flex-1 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5 text-sm"
              onChange={(e) => setForm({ ...form, password: e.target.value })}
              placeholder="Password"
              type="password"
              value={form.password}
            />
          </div>
          <div className="flex gap-2">
            <button
              className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm text-[var(--color-text-on-inverted)]"
              onClick={handleAdd}
            >
              Save
            </button>
            <button
              className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
              onClick={() => setAdding(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="overflow-x-auto rounded-lg border border-[var(--color-border-default)]">
        <table className="w-full min-w-[640px] text-left text-sm">
          <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
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
                className="border-b border-[var(--color-border-default)] last:border-0"
                key={account.address}
              >
                <td className="px-4 py-3 font-medium">
                  <Copyable value={account.address}>{account.address}</Copyable>
                </td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)] select-text">
                  {account.display_name}
                </td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                  {account.domain}
                </td>
                <td className="px-4 py-3">
                  <QuotaCell address={account.address} />
                </td>
                <td className="px-4 py-3">
                  {account.active ? (
                    <span className="inline-block h-2 w-2 rounded-full bg-[var(--color-status-success)]" />
                  ) : (
                    <span className="inline-block h-2 w-2 rounded-full bg-[var(--color-border-default)]" />
                  )}
                </td>
                <td className="px-4 py-3">
                  <PasswordCell account={account} onSaved={loadAccounts} />
                </td>
                <td className="px-4 py-3 text-right">
                  <div className="flex items-center justify-end gap-2">
                    <GroupsCell address={account.address} />
                    <SieveCell address={account.address} />
                    <button
                      className="text-xs text-[var(--color-status-danger)] transition-colors hover:opacity-70"
                      onClick={() => setDeleteTarget(account.address)}
                    >
                      Delete
                    </button>
                  </div>
                </td>
              </tr>
            ))}
            {filtered.length === 0 && (
              <tr>
                <td
                  className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
                  colSpan={7}
                >
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
        <div className="mt-4 flex items-center justify-between text-sm text-[var(--color-text-secondary)]">
          <span>
            Showing {safePage * PAGE_SIZE + 1}--
            {Math.min((safePage + 1) * PAGE_SIZE, filtered.length)} of{' '}
            {filtered.length}
          </span>
          <div className="flex gap-1">
            <button
              className="rounded-md px-2.5 py-1 hover:bg-[var(--color-hover)] disabled:opacity-40"
              disabled={safePage === 0}
              onClick={() => setPage((p) => Math.max(0, p - 1))}
            >
              Prev
            </button>
            <button
              className="rounded-md px-2.5 py-1 hover:bg-[var(--color-hover)] disabled:opacity-40"
              disabled={safePage >= totalPages - 1}
              onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
            >
              Next
            </button>
          </div>
        </div>
      )}

      {deleteTarget && (
        <DeleteConfirmDialog
          address={deleteTarget}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  )
}

function DeleteConfirmDialog({
  address,
  onCancel,
  onConfirm,
}: {
  address: string
  onCancel: () => void
  onConfirm: () => void
}) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onCancel])

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onCancel}
    >
      <div
        className="w-full max-w-sm rounded-lg bg-[var(--color-bg-raised)] p-6 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="mb-2 text-sm font-semibold">Confirm Deletion</h3>
        <p className="mb-4 text-sm text-[var(--color-text-tertiary)]">
          Are you sure you want to delete{' '}
          <span className="font-medium text-[var(--color-text-primary)]">
            {address}
          </span>
          ? This action cannot be undone.
        </p>
        <div className="flex justify-end gap-2">
          <button
            className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            className="rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90"
            onClick={onConfirm}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

function GroupsCell({ address }: { address: string }) {
  const [groups, setGroups] = useState<GroupInfo[] | null>(null)
  const [loading, setLoading] = useState(false)

  const load = async () => {
    setLoading(true)
    try {
      const data = await fetchJson<GroupInfo[]>(
        `/admin/accounts/${encodeURIComponent(address)}/groups`
      )
      setGroups(data)
    } catch {
      setGroups([])
    } finally {
      setLoading(false)
    }
  }

  if (groups === null && !loading) {
    return (
      <button
        className="text-xs text-[var(--color-brand-primary)] hover:opacity-80"
        onClick={load}
      >
        Groups
      </button>
    )
  }

  if (loading) {
    return (
      <span className="text-xs text-[var(--color-text-tertiary)]">
        Loading...
      </span>
    )
  }

  return (
    <div className="flex flex-wrap items-center gap-1">
      {groups && groups.length > 0 ? (
        groups.map((g) => (
          <span
            className="rounded-full bg-[var(--color-brand-subtle)] px-2 py-0.5 text-xs text-[var(--color-brand-primary)]"
            key={g.id}
            title={g.description}
          >
            {g.name}
          </span>
        ))
      ) : (
        <span className="text-xs text-[var(--color-text-tertiary)]">
          No groups
        </span>
      )}
      <button
        className="ml-1 text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
        onClick={() => setGroups(null)}
      >
        Close
      </button>
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
        display_name: account.display_name,
        domain: account.domain,
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
        className="text-xs text-[var(--color-brand-primary)] hover:opacity-80"
        onClick={() => setEditing(true)}
      >
        Change
      </button>
    )
  }

  return (
    <div className="flex items-center gap-1.5">
      <input
        autoFocus
        className="w-28 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 py-0.5 text-xs"
        disabled={saving}
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
        className="text-xs text-[var(--color-status-success)] hover:opacity-80 disabled:opacity-50"
        disabled={saving || !password.trim()}
        onClick={handleSave}
      >
        {saving ? '...' : 'Save'}
      </button>
      <button
        className="text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
        disabled={saving}
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

function QuotaCell({ address }: { address: string }) {
  const [quota, setQuota] = useState<QuotaState>({ status: 'loading' })

  useEffect(() => {
    let cancelled = false
    fetchJson<QuotaInfo>(`/admin/accounts/${encodeURIComponent(address)}/quota`)
      .then((data) => {
        if (!cancelled) {
          setQuota({ quotaBytes: data.quota_bytes, status: 'loaded' })
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
      <span className="text-xs text-[var(--color-text-tertiary)]">
        Loading...
      </span>
    )
  }

  if (quota.status === 'none') {
    return (
      <span className="text-xs text-[var(--color-text-tertiary)]">
        No quota set
      </span>
    )
  }

  const totalBytes = quota.quotaBytes
  const formatted = formatBytes(totalBytes)

  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 w-20 overflow-hidden rounded-full bg-[var(--color-border-default)]">
        <div className="h-full w-0 rounded-full bg-[var(--color-brand-primary)]" />
      </div>
      <span className="text-xs text-[var(--color-text-secondary)]">
        {formatted}
      </span>
    </div>
  )
}

function SieveCell({ address }: { address: string }) {
  const [state, setState] = useState<SieveState>({
    error: null,
    script: '',
    status: 'idle',
  })

  const load = async () => {
    setState({ error: null, script: '', status: 'loading' })
    try {
      const data = await fetchJson<{ script: string }>(
        `/admin/accounts/${encodeURIComponent(address)}/sieve`
      )
      setState({ error: null, script: data.script ?? '', status: 'loaded' })
    } catch {
      setState({ error: null, script: '', status: 'loaded' })
    }
  }

  const save = async () => {
    setState((prev) => ({ ...prev, error: null, status: 'saving' }))
    try {
      await postJson(`/admin/accounts/${encodeURIComponent(address)}/sieve`, {
        script: state.script,
      })
      toast.success('Sieve script saved')
      setState((prev) => ({ ...prev, error: null, status: 'loaded' }))
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to save'
      toast.error(msg)
      setState((prev) => ({ ...prev, error: msg, status: 'loaded' }))
    }
  }

  const remove = async () => {
    if (!confirm('Delete this sieve script? This cannot be undone.')) return
    setState((prev) => ({ ...prev, error: null, status: 'deleting' }))
    try {
      await deleteJson(`/admin/accounts/${encodeURIComponent(address)}/sieve`)
      toast.success('Sieve script deleted')
      setState({ error: null, script: '', status: 'idle' })
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete')
      setState((prev) => ({ ...prev, status: 'loaded' }))
    }
  }

  if (state.status === 'idle') {
    return (
      <button
        className="text-xs text-[var(--color-brand-primary)] hover:opacity-80"
        onClick={load}
      >
        Sieve
      </button>
    )
  }

  if (state.status === 'loading') {
    return (
      <span className="text-xs text-[var(--color-text-tertiary)]">
        Loading...
      </span>
    )
  }

  return (
    <div className="space-y-2">
      <textarea
        className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 py-1.5 font-mono text-xs"
        disabled={state.status === 'saving' || state.status === 'deleting'}
        onChange={(e) =>
          setState((prev) => ({ ...prev, script: e.target.value }))
        }
        placeholder='require "fileinto"; ...'
        rows={6}
        value={state.script}
      />
      {state.error && (
        <p className="text-xs text-[var(--color-status-danger)]">
          {state.error}
        </p>
      )}
      <div className="flex gap-1.5">
        <button
          className="text-xs text-[var(--color-status-success)] hover:opacity-80 disabled:opacity-50"
          disabled={state.status === 'saving'}
          onClick={save}
        >
          {state.status === 'saving' ? '...' : 'Save'}
        </button>
        <button
          className="text-xs text-[var(--color-status-danger)] hover:opacity-80 disabled:opacity-50"
          disabled={state.status === 'deleting'}
          onClick={remove}
        >
          {state.status === 'deleting' ? '...' : 'Delete'}
        </button>
        <button
          className="text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
          onClick={() => setState({ error: null, script: '', status: 'idle' })}
        >
          Close
        </button>
      </div>
    </div>
  )
}
