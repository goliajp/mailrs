import { Button } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { ScrollText } from 'lucide-react'
import { useMemo, useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { ScrollableTable } from '@/components/scrollable-table'
import { fetchList } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

type AuditEntry = {
  action: string
  actor: string
  detail: string
  id: number
  target: string
  timestamp: number
}

const HEADERS = ['Time', 'Actor', 'Action', 'Target', 'Detail']

// Group actions by resource prefix for the filter dropdown. Derived from the
// canonical list in the audit-log retrofit RFC (`<resource>_<verb>` plus
// the legacy `audit.*` family that wasn't renamed).
const ACTION_GROUPS = [
  { label: 'all', value: '' },
  { label: 'auth (login / password / totp / oidc)', value: 'auth' },
  { label: 'domain', value: 'domain_' },
  { label: 'account', value: 'account_' },
  { label: 'alias', value: 'alias_' },
  { label: 'group', value: 'group_' },
  { label: 'email_group', value: 'email_group_' },
  { label: 'app', value: 'app_' },
  { label: 'oauth_client', value: 'oauth_client_' },
  { label: 'sieve', value: 'sieve_' },
  { label: 'queue / suppression', value: 'queue_or_suppression' },
  { label: 'cache', value: 'cache_' },
  { label: 'system_config', value: 'system_config_' },
  { label: 'greylist', value: 'greylist_local_' },
  { label: "audit-read (read someone else's mail)", value: 'audit.' },
]

const AUTH_ACTIONS = new Set([
  'login',
  'login_failed',
  'oidc_login',
  'password_changed',
  'password_reset',
  'password_reset_requested',
  'recovery_code_used',
  'recovery_email_updated',
  'totp_disabled',
  'totp_enabled',
  'totp_failed',
  'totp_setup',
])

export function AdminAuditLog() {
  const [actionFilter, setActionFilter] = useState('')
  const [actorFilter, setActorFilter] = useState('')

  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.auditLog(),
    queryFn: ({ signal }) => fetchList<AuditEntry>('/admin/audit-log?limit=200', signal),
  })
  const entries = useMemo(() => data ?? [], [data])

  const filtered = useMemo(
    () =>
      entries.filter(
        (e) =>
          matchesActionFilter(e.action, actionFilter) &&
          (actorFilter === '' || e.actor.toLowerCase().includes(actorFilter.toLowerCase()))
      ),
    [entries, actionFilter, actorFilter]
  )

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
      title="Audit Log"
    >
      <div className="border-border mb-4 flex flex-wrap items-center gap-2 rounded-lg border p-3 text-sm">
        <label className="text-fg-muted text-xs">Action</label>
        <select
          aria-label="Filter by action"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setActionFilter(e.target.value)}
          value={actionFilter}
        >
          {ACTION_GROUPS.map((g) => (
            <option key={g.value} value={g.value}>
              {g.label}
            </option>
          ))}
        </select>
        <label className="text-fg-muted ml-2 text-xs">Actor</label>
        <input
          aria-label="Filter by actor address (substring)"
          className="border-border bg-bg-secondary rounded-md border px-2 py-1 text-sm"
          onChange={(e) => setActorFilter(e.target.value)}
          placeholder="user@example.com"
          value={actorFilter}
        />
        <span className="text-fg-muted ml-auto text-xs">
          {filtered.length} / {entries.length}
        </span>
      </div>

      {isPending ? (
        <AdminTableSkeleton cols={5} headers={HEADERS} rows={6} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} retryDisabled={isFetching} />
      ) : filtered.length === 0 ? (
        <AdminEmptyState
          description={
            entries.length === 0
              ? 'Actions taken by admins will appear here.'
              : 'No entries match the current filters.'
          }
          icon={<ScrollText className="h-10 w-10" />}
          title={entries.length === 0 ? 'No audit log entries' : 'No matches'}
        />
      ) : (
        <ScrollableTable>
          <table className="w-full text-left text-sm">
            <thead className="border-border bg-bg-secondary border-b">
              <tr>
                {HEADERS.map((h) => (
                  <th className="px-4 py-2.5 font-medium" key={h}>
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {filtered.map((entry) => (
                <tr className="border-border border-b last:border-0" key={entry.id}>
                  <td className="text-fg-secondary px-4 py-3 whitespace-nowrap">
                    {formatTime(entry.timestamp)}
                  </td>
                  <td className="px-4 py-3 font-medium">{entry.actor}</td>
                  <td className={`px-4 py-3 font-medium ${actionColor(entry.action)}`}>
                    {entry.action}
                  </td>
                  <td className="text-fg-secondary px-4 py-3">{entry.target}</td>
                  <td className="text-fg-muted max-w-xl px-4 py-3">
                    <DetailCell detail={entry.detail} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </ScrollableTable>
      )}
    </AdminPageShell>
  )
}

function actionColor(action: string): string {
  if (action === 'login_failed' || action === 'totp_failed') return 'text-danger'
  if (action === 'login' || action === 'oidc_login') return 'text-success'
  if (action.startsWith('audit.')) return 'text-warning'
  return 'text-fg'
}

function DetailCell({ detail }: { detail: string }) {
  const [expanded, setExpanded] = useState(false)
  const trimmed = detail.trim()
  const looksLikeJson = trimmed.startsWith('{') && trimmed.endsWith('}')

  if (!looksLikeJson) {
    return <span className="block truncate">{detail}</span>
  }

  let pretty: null | string
  try {
    pretty = JSON.stringify(JSON.parse(trimmed), null, 2)
  } catch {
    pretty = null
  }

  if (pretty === null) {
    return <span className="block truncate">{detail}</span>
  }

  return (
    <div>
      <button
        className="text-fg-secondary hover:text-fg block max-w-full truncate text-left"
        onClick={() => setExpanded((s) => !s)}
        type="button"
      >
        {expanded ? '▼ collapse' : '▶ JSON'} ·{' '}
        <span className="font-mono">{trimmed.slice(0, 60)}…</span>
      </button>
      {expanded ? (
        <pre className="bg-bg-secondary text-fg mt-1 max-w-full overflow-auto rounded-md p-2 font-mono text-xs">
          {pretty}
        </pre>
      ) : null}
    </div>
  )
}

function formatTime(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString()
}

function matchesActionFilter(action: string, filter: string): boolean {
  if (!filter) return true
  if (filter === 'auth') return AUTH_ACTIONS.has(action)
  if (filter === 'queue_or_suppression') {
    return action.startsWith('queue_') || action.startsWith('suppression_')
  }
  return action.startsWith(filter)
}
