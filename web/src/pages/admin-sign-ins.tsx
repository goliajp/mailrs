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
import { type Key, useT } from '@/i18n'
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
const HEADERS: Key[] = [
  'admin.time',
  'admin.signIns.account',
  'admin.aliases.source',
  'admin.signIns.wayIn',
  'admin.signIns.result',
  'admin.signIns.client',
]
const NO_ROWS: SignIn[] = []

const RESULTS: { label: Key; value: string }[] = [
  { label: 'admin.signIns.result.any', value: '' },
  { label: 'admin.signIns.result.ok', value: 'ok' },
  { label: 'admin.signIns.result.badPassword', value: 'bad-password' },
  { label: 'admin.signIns.result.locked', value: 'locked' },
  { label: 'admin.signIns.result.mfaRequired', value: 'mfa-required' },
  { label: 'admin.signIns.result.mfaFailed', value: 'mfa-failed' },
]

const CHANNELS: { label: Key; value: string }[] = [
  { label: 'admin.signIns.channel.any', value: '' },
  { label: 'admin.signIns.channel.web', value: 'web' },
  { label: 'admin.signIns.channel.verify', value: 'verify' },
  { label: 'admin.proto.imap', value: 'imap' },
  { label: 'admin.proto.pop3', value: 'pop3' },
  { label: 'admin.proto.smtp', value: 'smtp' },
  { label: 'admin.proto.managesieve', value: 'managesieve' },
  { label: 'admin.signIns.channel.dav', value: 'dav' },
  { label: 'admin.signIns.channel.apiKey', value: 'api-key' },
]

export function AdminSignIns() {
  const t = useT()
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
      header: t('admin.time'),
      key: 'time',
      cell: (r) => (
        <span className="text-fg-secondary whitespace-nowrap" title={formatFullDate(r.occurred_at)}>
          {formatDate(r.occurred_at)}
        </span>
      ),
    },
    {
      header: t('admin.signIns.account'),
      key: 'account',
      cell: (r) => <span className="font-medium break-all">{r.account || '—'}</span>,
    },
    {
      header: t('admin.aliases.source'),
      key: 'source',
      cell: (r) => (
        <button
          className="text-accent font-mono text-xs hover:opacity-80"
          onClick={() => setParam('source', r.source_ip)}
          title={t('admin.signIns.onlySource')}
          type="button"
        >
          {r.source_ip}
        </button>
      ),
    },
    {
      header: t('admin.signIns.wayIn'),
      key: 'channel',
      cell: (r) => (
        <span className="text-fg-secondary text-xs">
          {r.channel} · {r.credential}
        </span>
      ),
    },
    {
      header: t('admin.signIns.result'),
      key: 'result',
      cell: (r) => <span className={`font-mono text-xs ${resultColor(r.result)}`}>{r.result}</span>,
    },
    {
      header: t('admin.signIns.client'),
      key: 'client',
      cell: (r) => (
        <span className="text-fg-muted block max-w-xs truncate text-xs" title={r.user_agent}>
          {r.key_id ?? r.user_agent ?? '—'}
        </span>
      ),
    },
  ]

  const renderBody = () => {
    if (isPending) {
      return <AdminTableSkeleton cols={6} headers={HEADERS.map((h) => t(h))} rows={6} />
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          retryDisabled={isFetching}
          what={t('admin.signIns.listName')}
        />
      )
    }
    if (rows.length === 0) {
      return (
        <AdminEmptyState
          description={t('admin.signIns.emptyHelp')}
          icon={<KeyRound className="h-10 w-10" />}
          title={t('admin.signIns.empty')}
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        label={t('admin.signIns.title')}
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
          {t('admin.refresh')}
        </Button>
      }
      subtitle={t('admin.signIns.subtitle', { n: LIMIT })}
      title={t('admin.signIns.title')}
    >
      <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
        <input
          aria-label={t('admin.signIns.filterAccount')}
          className="border-border bg-bg-secondary w-64 max-w-full rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setParam('account', e.target.value)}
          placeholder={t('admin.signIns.accountPlaceholder')}
          value={filters.account}
        />
        <input
          aria-label={t('admin.signIns.filterSource')}
          className="border-border bg-bg-secondary w-44 max-w-full rounded-md border px-3 py-1.5 font-mono text-sm"
          onChange={(e) => setParam('source', e.target.value)}
          placeholder={t('admin.signIns.sourceAddress')}
          value={filters.source}
        />
        <select
          aria-label={t('admin.signIns.filterResult')}
          className="border-border bg-bg-secondary rounded-md border px-2 py-1.5 text-sm"
          onChange={(e) => setParam('result', e.target.value)}
          value={filters.result}
        >
          {RESULTS.map((o) => (
            <option key={o.value} value={o.value}>
              {t(o.label)}
            </option>
          ))}
        </select>
        <select
          aria-label={t('admin.signIns.filterWayIn')}
          className="border-border bg-bg-secondary rounded-md border px-2 py-1.5 text-sm"
          onChange={(e) => setParam('channel', e.target.value)}
          value={filters.channel}
        >
          {CHANNELS.map((o) => (
            <option key={o.value} value={o.value}>
              {t(o.label)}
            </option>
          ))}
        </select>
        <span className="text-fg-muted ml-auto text-xs tabular-nums">
          {t('admin.nShown', { n: rows.length })}
        </span>
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
