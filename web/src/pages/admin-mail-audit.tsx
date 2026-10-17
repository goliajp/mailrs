import { useQuery } from '@tanstack/react-query'
import { Search, Users } from 'lucide-react'
import { useMemo } from 'react'
import { useSearchParams } from 'react-router'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
} from '@/components/admin-page'
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
      subtitle="Reading another account's mail is recorded: each conversation list, each conversation opened and each .eml download writes a row to the audit log under your address"
      title="Mail audit"
    >
      <input
        aria-label="Search accounts"
        className="border-border bg-bg-secondary mb-4 w-64 max-w-full rounded-md border px-3 py-1.5 text-sm"
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Address or name"
        type="search"
        value={search}
      />
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
  if (error) return <AdminErrorState error={error} onRetry={onRetry} what="the account list" />
  if (accounts.length === 0) {
    return (
      <AdminEmptyState
        description="Accounts added on the Accounts page can be reviewed here"
        icon={<Users className="h-10 w-10" />}
        title="No accounts yet"
      />
    )
  }
  if (filtered.length === 0) {
    return (
      <AdminEmptyState
        description="Search matches part of an address or a display name"
        icon={<Search className="h-10 w-10" />}
        title={`No accounts match "${search}"`}
      />
    )
  }
  const columns: Column<AuditAccount>[] = [
    {
      header: 'Account',
      key: 'address',
      cell: (a) => <span className="font-medium">{a.address}</span>,
    },
    { header: 'Domain', key: 'domain', cell: (a) => <Muted text={a.domain} /> },
    { header: 'Name', key: 'name', cell: (a) => <Muted text={a.display_name || '—'} /> },
    { header: 'Status', key: 'status', cell: (a) => <ActiveLabel active={a.active} /> },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (a) => (
        <button
          className="text-accent text-xs hover:opacity-80"
          onClick={() => onView(a.address)}
          type="button"
        >
          View mail
        </button>
      ),
    },
  ]
  return <AdminTable columns={columns} label="Accounts" rowKey={(a) => a.address} rows={filtered} />
}

function ActiveLabel({ active }: { active: boolean }) {
  if (active) return <span className="text-success text-xs">Active</span>
  return <span className="text-fg-muted text-xs">Inactive</span>
}

function Muted({ text }: { text: string }) {
  return <span className="text-fg-secondary">{text}</span>
}
