import { useQuery } from '@tanstack/react-query'
import { Search, Users } from 'lucide-react'
import { useMemo } from 'react'
import { useSearchParams } from 'react-router'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { ScrollableTable } from '@/components/scrollable-table'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet } from '@/wire/endpoints/admin'

import { AuditConversations, AuditThread } from './admin-mail-audit-views'

type AuditAccount = {
  active: boolean
  address: string
  display_name: string
  domain: string
}

const ACCOUNT_HEADERS = ['Account', 'Domain', 'Name', 'Status', '']

export function AdminMailAudit() {
  const [searchParams, setSearchParams] = useSearchParams()
  const selectedAccount = searchParams.get('address')
  const selectedThread = searchParams.get('thread')
  const search = searchParams.get('q') ?? ''

  const setSelectedAccount = (address: null | string) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (address) next.set('address', address)
      else next.delete('address')
      next.delete('thread')
      return next
    })
  }

  const setSelectedThread = (threadId: null | string) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (threadId) next.set('thread', threadId)
      else next.delete('thread')
      return next
    })
  }

  const setSearch = (q: string) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (q) next.set('q', q)
      else next.delete('q')
      return next
    })
  }

  const {
    data: accountsData,
    error: accountsError,
    isPending: accountsPending,
    refetch: refetchAccounts,
  } = useQuery({
    queryKey: adminKeys.mailAuditAccounts(),
    queryFn: ({ signal }) => adminListGet<AuditAccount>('/admin/audit/accounts', signal),
  })
  const accounts = useMemo(() => accountsData ?? [], [accountsData])

  const filteredAccounts = useMemo(() => {
    if (!search) return accounts
    const q = search.toLowerCase()
    return accounts.filter(
      (a) => a.address.toLowerCase().includes(q) || a.display_name.toLowerCase().includes(q)
    )
  }, [accounts, search])

  if (selectedAccount && selectedThread) {
    return (
      <AuditThread
        account={selectedAccount}
        onBack={() => setSelectedThread(null)}
        threadId={selectedThread}
      />
    )
  }
  if (selectedAccount) {
    return (
      <AuditConversations
        account={selectedAccount}
        onBack={() => setSelectedAccount(null)}
        onOpen={setSelectedThread}
      />
    )
  }

  return (
    <AdminPageShell
      subtitle="Select an account to review its mail; every read is written to the audit log"
      title="Mail Audit"
    >
      <div className="mb-4 flex items-center gap-2">
        <div className="relative flex-1">
          <Search className="text-fg-muted absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2" />
          <input
            aria-label="Search accounts"
            className="border-border bg-bg focus:border-accent w-full rounded-lg border py-2 pr-3 pl-9 text-sm outline-none"
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search accounts…"
            type="text"
            value={search}
          />
        </div>
      </div>
      <AccountList
        accounts={accounts}
        error={accountsError}
        filtered={filteredAccounts}
        onRetry={() => refetchAccounts()}
        onView={setSelectedAccount}
        pending={accountsPending}
        search={search}
      />
    </AdminPageShell>
  )
}

function AccountList({
  accounts,
  error,
  filtered,
  onRetry,
  onView,
  pending,
  search,
}: {
  accounts: AuditAccount[]
  error: Error | null
  filtered: AuditAccount[]
  onRetry: () => void
  onView: (address: string) => void
  pending: boolean
  search: string
}) {
  if (pending) return <AdminTableSkeleton cols={5} headers={ACCOUNT_HEADERS} rows={6} />
  if (error) return <AdminErrorState error={error} onRetry={onRetry} />
  if (accounts.length === 0) {
    return (
      <AdminEmptyState
        description="No accounts are registered on this server"
        icon={<Users className="h-10 w-10" />}
        title="No auditable accounts"
      />
    )
  }
  if (filtered.length === 0) {
    return (
      <AdminEmptyState
        description={`No accounts match "${search}"`}
        icon={<Search className="h-10 w-10" />}
        title="No matches"
      />
    )
  }
  return (
    <ScrollableTable>
      <table className="w-full text-left text-sm">
        <thead className="border-border bg-bg-secondary border-b">
          <tr>
            {ACCOUNT_HEADERS.map((h, i) => (
              <th className="px-4 py-2.5 font-medium" key={i}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {filtered.map((a) => (
            <tr
              className="border-border hover:bg-bg-secondary border-b last:border-0"
              key={a.address}
            >
              <td className="px-4 py-3 font-medium">{a.address}</td>
              <td className="text-fg-secondary px-4 py-3">{a.domain}</td>
              <td className="text-fg-secondary px-4 py-3">{a.display_name || '—'}</td>
              <td className="px-4 py-3">
                <ActiveBadge active={a.active} />
              </td>
              <td className="px-4 py-3">
                <button
                  className="bg-fg text-bg rounded-md px-3 py-1 text-xs font-medium transition-colors hover:opacity-90"
                  onClick={() => onView(a.address)}
                >
                  View Mail
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollableTable>
  )
}

function ActiveBadge({ active }: { active: boolean }) {
  if (active) {
    return (
      <span className="bg-success/10 text-success rounded-full px-2 py-0.5 text-xs font-medium">
        Active
      </span>
    )
  }
  return (
    <span className="bg-bg-secondary text-fg-muted rounded-full px-2 py-0.5 text-xs font-medium">
      Inactive
    </span>
  )
}
