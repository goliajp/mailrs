import { Button } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { KeyRound } from 'lucide-react'
import { useSearchParams } from 'react-router'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
} from '@/components/admin-page'
import { formatDate, formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet } from '@/wire/endpoints/admin'

// Backend: crates/webapi/src/handlers/admin_auth_events.rs — fn list_auth_events
// (rows are mailrs_auth_guard::events::LoginEvent)
type SignIn = {
  account: string
  channel: string
  credential: string
  key_id?: string
  occurred_at: number
  result: string
  source_ip: string
  user_agent?: string
}

const LIMIT = 200
const FILTERS = ['account', 'source', 'result', 'channel'] as const
const HEADERS = ['Time', 'Account', 'Source', 'Way in', 'Result', 'Client']
const NO_ROWS: SignIn[] = []

const RESULTS = [
  { label: 'Every result', value: '' },
  { label: 'Signed in', value: 'ok' },
  { label: 'Wrong credentials', value: 'bad-password' },
  { label: 'Refused while locked', value: 'locked' },
  { label: 'Second factor asked', value: 'mfa-required' },
  { label: 'Second factor wrong', value: 'mfa-failed' },
]

const CHANNELS = [
  { label: 'Every way in', value: '' },
  { label: 'Web', value: 'web' },
  { label: 'Credential check', value: 'verify' },
  { label: 'IMAP', value: 'imap' },
  { label: 'POP3', value: 'pop3' },
  { label: 'SMTP', value: 'smtp' },
  { label: 'ManageSieve', value: 'managesieve' },
  { label: 'Calendar and contacts', value: 'dav' },
  { label: 'API key', value: 'api-key' },
]

export function AdminSignIns() {
  const [params, setParams] = useSearchParams()
  const filters = Object.fromEntries(FILTERS.map((f) => [f, params.get(f) ?? '']))

  const setParam = (name: string, value: string) => {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev)
        if (value) next.set(name, value)
        else next.delete(name)
        return next
      },
      { replace: true }
    )
  }

  const query = new URLSearchParams({ limit: String(LIMIT) })
  for (const f of FILTERS) {
    if (filters[f]) query.set(f, filters[f].trim())
  }
  const qs = query.toString()

  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.signIns(qs),
    queryFn: ({ signal }) => adminListGet<SignIn>(`/admin/auth-events?${qs}`, signal),
  })
  const rows = data ?? NO_ROWS

  const columns: Column<SignIn>[] = [
    {
      header: 'Time',
      key: 'time',
      cell: (r) => (
        <span className="text-fg-secondary whitespace-nowrap" title={formatFullDate(r.occurred_at)}>
          {formatDate(r.occurred_at)}
        </span>
      ),
    },
    {
      header: 'Account',
      key: 'account',
      cell: (r) => <span className="font-medium break-all">{r.account || '—'}</span>,
    },
    {
      header: 'Source',
      key: 'source',
      cell: (r) => (
        <button
          className="text-accent font-mono text-xs hover:opacity-80"
          onClick={() => setParam('source', r.source_ip)}
          title="Show only this source"
          type="button"
        >
          {r.source_ip}
        </button>
      ),
    },
    {
      header: 'Way in',
      key: 'channel',
      cell: (r) => (
        <span className="text-fg-secondary text-xs">
          {r.channel} · {r.credential}
        </span>
      ),
    },
    {
      header: 'Result',
      key: 'result',
      cell: (r) => <span className={`font-mono text-xs ${resultColor(r.result)}`}>{r.result}</span>,
    },
    {
      header: 'Client',
      key: 'client',
      cell: (r) => (
        <span className="text-fg-muted block max-w-xs truncate text-xs" title={r.user_agent}>
          {r.key_id ?? r.user_agent ?? '—'}
        </span>
      ),
    },
  ]

  const renderBody = () => {
    if (isPending) return <AdminTableSkeleton cols={6} headers={HEADERS} rows={6} />
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          retryDisabled={isFetching}
          what="the sign-ins"
        />
      )
    }
    if (rows.length === 0) {
      return (
        <AdminEmptyState
          description="Every attempt to sign in, on the web, by mail apps or with a key, is recorded here and kept for 90 days. Change or clear the filters to see more"
          icon={<KeyRound className="h-10 w-10" />}
          title="No sign-ins match"
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        label="Sign-ins"
        rowKey={(r) => `${r.occurred_at}-${r.source_ip}-${r.account}-${r.channel}-${r.result}`}
        rows={rows}
      />
    )
  }

  return (
    <AdminPageShell
      actions={
        <Button
          disabled={isFetching}
          loading={isFetching}
          onClick={() => refetch()}
          size="sm"
          variant="secondary"
        >
          Refresh
        </Button>
      }
      subtitle={`The newest ${LIMIT} that match, newest first`}
      title="Sign-ins"
    >
      <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
        <input
          aria-label="Filter by account"
          className="border-border bg-bg-secondary w-64 max-w-full rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setParam('account', e.target.value)}
          placeholder="Account, e.g. lihao@golia.jp"
          value={filters.account}
        />
        <input
          aria-label="Filter by source address"
          className="border-border bg-bg-secondary w-44 max-w-full rounded-md border px-3 py-1.5 font-mono text-sm"
          onChange={(e) => setParam('source', e.target.value)}
          placeholder="Source address"
          value={filters.source}
        />
        <select
          aria-label="Filter by result"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1.5 text-sm"
          onChange={(e) => setParam('result', e.target.value)}
          value={filters.result}
        >
          {RESULTS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <select
          aria-label="Filter by way in"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1.5 text-sm"
          onChange={(e) => setParam('channel', e.target.value)}
          value={filters.channel}
        >
          {CHANNELS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <span className="text-fg-muted ml-auto text-xs tabular-nums">{rows.length} shown</span>
      </div>
      {renderBody()}
    </AdminPageShell>
  )
}

function resultColor(result: string): string {
  switch (result) {
    case 'locked':
      return 'text-danger'
    case 'mfa-required':
      return 'text-fg-secondary'
    case 'ok':
      return 'text-success'
    default:
      return 'text-warning'
  }
}
