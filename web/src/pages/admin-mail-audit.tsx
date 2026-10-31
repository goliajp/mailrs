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
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet } from '@/wire/endpoints/admin'

import { AuditConversations, AuditThread } from './admin-mail-audit-views'

type AuditAccount = {
  active: boolean
  address: string
  display_name: string
  domain: string
}

const ACCOUNT_HEADERS: Key[] = [
  'admin.signIns.account',
  'admin.col.domain',
  'admin.col.name',
  'admin.col.status',
]

export function AdminMailAudit() {
  const t = useT()
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
    <AdminPageShell subtitle={t('admin.mailAudit.subtitle')} title={t('admin.mailAudit.title')}>
      <input
        aria-label={t('admin.mailAudit.search')}
        className="border-border bg-bg-secondary mb-4 w-64 max-w-full rounded-md border px-3 py-1.5 text-sm"
        onChange={(e) => setSearch(e.target.value)}
        placeholder={t('admin.mailAudit.searchPlaceholder')}
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
  const t = useT()
  if (pending) {
    return (
      <AdminTableSkeleton cols={5} headers={[...ACCOUNT_HEADERS.map((h) => t(h)), '']} rows={6} />
    )
  }
  if (error) {
    return <AdminErrorState error={error} onRetry={onRetry} what={t('admin.accounts.listName')} />
  }
  if (accounts.length === 0) {
    return (
      <AdminEmptyState
        description={t('admin.mailAudit.emptyHelp')}
        icon={<Users className="h-10 w-10" />}
        title={t('admin.accounts.empty')}
      />
    )
  }
  if (filtered.length === 0) {
    return (
      <AdminEmptyState
        description={t('admin.mailAudit.searchHelp')}
        icon={<Search className="h-10 w-10" />}
        title={t('admin.accounts.noMatch', { filter: search })}
      />
    )
  }
  const columns: Column<AuditAccount>[] = [
    {
      header: t('admin.signIns.account'),
      key: 'address',
      cell: (a) => <span className="font-medium">{a.address}</span>,
    },
    { header: t('admin.col.domain'), key: 'domain', cell: (a) => <Muted text={a.domain} /> },
    {
      header: t('admin.col.name'),
      key: 'name',
      cell: (a) => <Muted text={a.display_name || '—'} />,
    },
    {
      header: t('admin.col.status'),
      key: 'status',
      cell: (a) => <ActiveLabel active={a.active} />,
    },
    {
      align: 'end',
      header: t('admin.col.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (a) => (
        <button
          className="text-accent text-xs hover:opacity-80"
          onClick={() => onView(a.address)}
          type="button"
        >
          {t('admin.mailAudit.viewMail')}
        </button>
      ),
    },
  ]
  return (
    <AdminTable
      columns={columns}
      label={t('admin.accounts.title')}
      rowKey={(a) => a.address}
      rows={filtered}
    />
  )
}

function ActiveLabel({ active }: { active: boolean }) {
  const t = useT()
  if (active) return <span className="text-success text-xs">{t('admin.active')}</span>
  return <span className="text-fg-muted text-xs">{t('admin.inactive')}</span>
}

function Muted({ text }: { text: string }) {
  return <span className="text-fg-secondary">{text}</span>
}
