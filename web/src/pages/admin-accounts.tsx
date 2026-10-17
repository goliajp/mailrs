import type { AccountPanel, OpenPanel } from '@/components/admin-accounts/account-columns'
import type { AccountInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Users } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'

import { GroupsPanel, SievePanel } from '@/components/admin-accounts'
import { accountColumns } from '@/components/admin-accounts/account-columns'
import { AddAccountForm } from '@/components/admin-accounts/add-account-form'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  ConfirmAction,
} from '@/components/admin-page'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminListGet } from '@/wire/endpoints/admin'

const PAGE_SIZE = 20
const HEADERS = ['Address', 'Display name', 'Domain', 'Quota', 'Status', 'Password', '']

export function AdminAccounts() {
  const [searchParams, setSearchParams] = useSearchParams()
  const filter = searchParams.get('q') ?? ''
  const page = Math.max(0, Number.parseInt(searchParams.get('page') ?? '0', 10) || 0)
  const deleteTarget = searchParams.get('delete')

  const setParam = (key: string, value: null | string) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (value) next.set(key, value)
      else next.delete(key)
      if (key === 'q') next.delete('page')
      return next
    })
  }

  const { data, error, isPending, refetch } = useQuery({
    queryKey: adminKeys.accounts(),
    queryFn: ({ signal }) => adminListGet<AccountInfo>('/admin/accounts', signal),
  })
  const accounts = useMemo(() => data ?? [], [data])

  const [adding, setAdding] = useState(false)
  const [open, setOpen] = useState<OpenPanel>(null)

  const deleteAccount = useAdminMutation({
    invalidateKey: adminKeys.accounts(),
    mutationFn: (address: string) => adminDelete(`/admin/accounts/${encodeURIComponent(address)}`),
    successMsg: (address) => `Account "${address}" deleted`,
  })

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

  const toggle = (address: string, panel: AccountPanel) => {
    if (open?.address === address && open.panel === panel) setOpen(null)
    else setOpen({ address, panel })
  }

  const columns = accountColumns({
    onToggle: toggle,
    open,
    onDelete: (address) => setParam('delete', address),
  })

  const panelOf = (a: AccountInfo) => {
    if (open?.address !== a.address) return null
    if (open.panel === 'groups') return <GroupsPanel address={a.address} />
    return <SievePanel address={a.address} onClose={() => setOpen(null)} />
  }

  const renderList = () => {
    if (isPending) return <AdminTableSkeleton cols={7} headers={HEADERS} rows={6} />
    if (error) {
      return <AdminErrorState error={error} onRetry={() => refetch()} what="the account list" />
    }
    if (accounts.length === 0 && !adding) {
      return (
        <AdminEmptyState
          description="An account is a mailbox someone signs in to, over the web, IMAP or POP3. Add one to start receiving mail at its address"
          icon={<Users className="h-10 w-10" />}
          title="No accounts yet"
        />
      )
    }
    if (filtered.length === 0) {
      return <AdminEmptyState description={`No accounts match "${filter}"`} title="No matches" />
    }
    return (
      <>
        <AdminTable
          columns={columns}
          detail={panelOf}
          label="Accounts"
          rowKey={(a) => a.address}
          rows={paged}
        />
        {totalPages > 1 && (
          <div className="text-fg-secondary mt-4 flex items-center justify-between text-sm">
            <span className="tabular-nums">
              Showing {safePage * PAGE_SIZE + 1}–
              {Math.min((safePage + 1) * PAGE_SIZE, filtered.length)} of {filtered.length}
            </span>
            <div className="flex gap-1">
              <button
                className="hover:bg-bg-secondary rounded-md px-2.5 py-1 disabled:opacity-40"
                disabled={safePage === 0}
                onClick={() => setParam('page', pageParam(safePage - 1))}
              >
                Prev
              </button>
              <button
                className="hover:bg-bg-secondary rounded-md px-2.5 py-1 disabled:opacity-40"
                disabled={safePage >= totalPages - 1}
                onClick={() => setParam('page', pageParam(safePage + 1))}
              >
                Next
              </button>
            </div>
          </div>
        )}
      </>
    )
  }

  return (
    <AdminPageShell
      actions={
        <div className="flex items-center gap-3">
          <input
            aria-label="Filter accounts"
            className="border-border bg-bg-secondary placeholder:text-fg-muted focus:border-accent w-64 rounded-md border px-3 py-1.5 text-sm outline-none"
            onChange={(e) => setParam('q', e.target.value)}
            placeholder="Filter accounts..."
            value={filter}
          />
          {!adding && (
            <button
              className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
              onClick={() => setAdding(true)}
            >
              Add account
            </button>
          )}
        </div>
      }
      title="Accounts"
    >
      {adding && <AddAccountForm onDone={() => setAdding(false)} />}

      {renderList()}

      {deleteTarget && (
        <ConfirmAction
          confirmLabel={pendingLabel(deleteAccount.isPending, 'Deleting...', 'Delete account')}
          consequence={accountDeleteConsequence(deleteTarget)}
          onCancel={() => setParam('delete', null)}
          onConfirm={() =>
            deleteAccount.mutate(deleteTarget, { onSettled: () => setParam('delete', null) })
          }
          pending={deleteAccount.isPending}
          title={`Delete ${deleteTarget}?`}
        />
      )}
    </AdminPageShell>
  )
}

/**
 * Deleting removes the account record and its own permissions only. The
 * maildir stays, and delivery checks the maildir before the account, so
 * mail keeps arriving there. Open sessions are not revoked.
 */
function accountDeleteConsequence(address: string): string {
  return `${address} can no longer sign in, though sessions already open are not ended. Its stored mail stays on disk, and new mail to the address is still delivered into that mailbox. Its aliases and group memberships are kept`
}

function pageParam(p: number): null | string {
  if (p === 0) return null
  return String(p)
}

function pendingLabel(pending: boolean, busy: string, idle: string): string {
  if (pending) return busy
  return idle
}
