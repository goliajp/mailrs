import type { AccountInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Users } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'

import {
  DisplayNameCell,
  GroupsCell,
  PasswordCell,
  QuotaCell,
  SieveCell,
} from '@/components/admin-accounts'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { Copyable } from '@/components/copy-button'
import { MobileModal } from '@/components/mobile-modal'
import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

const PAGE_SIZE = 20
const HEADERS = ['Address', 'Display Name', 'Domain', 'Quota', 'Status', 'Password', 'Actions']

export function AdminAccounts() {
  const [searchParams, setSearchParams] = useSearchParams()
  const filter = searchParams.get('q') ?? ''
  const page = Math.max(0, Number.parseInt(searchParams.get('page') ?? '0', 10) || 0)

  const setFilter = (q: string) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (q) next.set('q', q)
      else next.delete('q')
      next.delete('page')
      return next
    })
  }

  const setPage = (p: number) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (p === 0) next.delete('page')
      else next.set('page', String(p))
      return next
    })
  }

  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.accounts(),
    queryFn: ({ signal }) => fetchJson<AccountInfo[]>('/admin/accounts', signal),
  })
  const accounts = useMemo(() => data ?? [], [data])

  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({
    address: '',
    displayName: '',
    domain: '',
    password: '',
  })

  const addAccount = useAdminMutation({
    invalidateKey: adminKeys.accounts(),
    mutationFn: (vars: {
      address: string
      displayName: string
      domain: string
      password: string
    }) =>
      postJson<{ message?: string; success: boolean }>('/admin/accounts', {
        address: vars.address,
        display_name: vars.displayName,
        domain: vars.domain,
        password: vars.password,
      }),
    successMsg: (vars) => `Account "${vars.address}" added`,
  })

  const deleteAccount = useAdminMutation({
    invalidateKey: adminKeys.accounts(),
    mutationFn: (address: string) => deleteJson(`/admin/accounts/${encodeURIComponent(address)}`),
    successMsg: (address) => `Account "${address}" removed`,
  })

  const handleAdd = () => {
    const local = form.address.trim().replace(/@.*$/, '')
    const domain = form.domain.trim()
    if (!local || !domain) return
    const fullAddress = `${local}@${domain}`
    addAccount.mutate(
      {
        address: fullAddress,
        displayName: form.displayName.trim(),
        domain,
        password: form.password,
      },
      {
        onSuccess: () => {
          setForm({ address: '', displayName: '', domain: '', password: '' })
          setAdding(false)
        },
      }
    )
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
  const deleteTarget = searchParams.get('delete')

  return (
    <AdminPageShell
      actions={
        <div className="flex items-center gap-3">
          <input
            aria-label="Filter accounts"
            className="border-border bg-bg-secondary placeholder:text-fg-muted focus:border-accent w-64 rounded-md border px-3 py-1.5 text-sm outline-none"
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter accounts..."
            value={filter}
          />
          {!adding && (
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
              onClick={() => setAdding(true)}
            >
              Add Account
            </button>
          )}
        </div>
      }
      title="Accounts"
    >
      {adding && (
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
              placeholder="Display Name"
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
              disabled={!form.address.trim() || !form.domain.trim() || addAccount.isPending}
              onClick={handleAdd}
            >
              {addAccount.isPending ? 'Saving...' : 'Save'}
            </button>
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
              onClick={() => {
                setForm({ address: '', displayName: '', domain: '', password: '' })
                setAdding(false)
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {isPending ? (
        <AdminTableSkeleton cols={7} headers={HEADERS} rows={6} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} />
      ) : accounts.length === 0 && !adding ? (
        <AdminEmptyState
          description="Create an account to start delivering mail to a mailbox."
          icon={<Users className="h-10 w-10" />}
          title="No accounts configured"
        />
      ) : filtered.length === 0 ? (
        <AdminEmptyState description={`No accounts match "${filter}".`} title="No matches" />
      ) : (
        <>
          <ScrollableTable>
            <table className="w-full min-w-[640px] text-left text-sm">
              <thead className="border-border bg-bg-secondary border-b">
                <tr>
                  {HEADERS.slice(0, -1).map((h) => (
                    <th className="px-4 py-2.5 font-medium" key={h}>
                      {h}
                    </th>
                  ))}
                  <th className="px-4 py-2.5 text-right font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {paged.map((account) => (
                  <tr className="border-border border-b last:border-0" key={account.address}>
                    <td className="px-4 py-3 font-medium">
                      <Copyable value={account.address}>{account.address}</Copyable>
                    </td>
                    <td className="px-4 py-3">
                      <DisplayNameCell account={account} />
                    </td>
                    <td className="text-fg-secondary px-4 py-3">{account.domain}</td>
                    <td className="px-4 py-3">
                      <QuotaCell address={account.address} />
                    </td>
                    <td className="px-4 py-3">
                      <span
                        className={`inline-block h-2 w-2 rounded-full ${account.active ? 'bg-success' : 'bg-border'}`}
                        title={account.active ? 'Active' : 'Inactive'}
                      />
                    </td>
                    <td className="px-4 py-3">
                      <PasswordCell account={account} />
                    </td>
                    <td className="px-4 py-3 text-right">
                      <div className="flex items-center justify-end gap-2">
                        <GroupsCell address={account.address} />
                        <SieveCell address={account.address} />
                        <button
                          className="text-danger text-xs transition-colors hover:opacity-70"
                          onClick={() => {
                            setSearchParams((prev) => {
                              const next = new URLSearchParams(prev)
                              next.set('delete', account.address)
                              return next
                            })
                          }}
                        >
                          Delete
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </ScrollableTable>

          {totalPages > 1 && (
            <div className="text-fg-secondary mt-4 flex items-center justify-between text-sm">
              <span>
                Showing {safePage * PAGE_SIZE + 1}–
                {Math.min((safePage + 1) * PAGE_SIZE, filtered.length)} of {filtered.length}
              </span>
              <div className="flex gap-1">
                <button
                  className="hover:bg-bg-secondary rounded-md px-2.5 py-1 disabled:opacity-40"
                  disabled={safePage === 0}
                  onClick={() => setPage(safePage - 1)}
                >
                  Prev
                </button>
                <button
                  className="hover:bg-bg-secondary rounded-md px-2.5 py-1 disabled:opacity-40"
                  disabled={safePage >= totalPages - 1}
                  onClick={() => setPage(safePage + 1)}
                >
                  Next
                </button>
              </div>
            </div>
          )}
        </>
      )}

      {deleteTarget && (
        <MobileModal
          onClose={() =>
            setSearchParams((prev) => {
              const next = new URLSearchParams(prev)
              next.delete('delete')
              return next
            })
          }
          open
        >
          <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
            <h3 className="mb-2 text-sm font-semibold">Confirm Deletion</h3>
            <p className="text-fg-muted mb-4 text-sm">
              Are you sure you want to delete{' '}
              <span className="text-fg font-medium">{deleteTarget}</span>? This action cannot be
              undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
                onClick={() =>
                  setSearchParams((prev) => {
                    const next = new URLSearchParams(prev)
                    next.delete('delete')
                    return next
                  })
                }
              >
                Cancel
              </button>
              <button
                className="bg-danger rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
                disabled={deleteAccount.isPending}
                onClick={() => {
                  deleteAccount.mutate(deleteTarget, {
                    onSettled: () => {
                      setSearchParams((prev) => {
                        const next = new URLSearchParams(prev)
                        next.delete('delete')
                        return next
                      })
                    },
                  })
                }}
              >
                {deleteAccount.isPending ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}
