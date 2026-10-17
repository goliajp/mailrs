import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { adminKeys } from '@/lib/query-keys'
import { adminListGet } from '@/wire/endpoints/admin'

type AuditEntry = {
  action: string
  actor: string
  id: number
  target: string
  timestamp: number
}

const EMPTY: AuditEntry[] = []

/** The last few things an admin or the system changed. */
export function RecentAudit() {
  const audit = useQuery({
    queryKey: adminKeys.overviewAuditLog(),
    refetchInterval: 30_000,
    queryFn: ({ signal }) => adminListGet<AuditEntry>('/admin/audit-log?limit=8', signal),
  })
  const rows = audit.data ?? EMPTY
  return (
    <section aria-label="Recent changes">
      <div className="mb-2 flex items-baseline justify-between">
        <h3 className="text-fg text-sm font-medium">Recent changes</h3>
        <Link className="text-fg-muted hover:text-fg text-xs" to="/admin/audit-log">
          Audit log
        </Link>
      </div>
      <Rows error={audit.error} rows={rows} />
    </section>
  )
}

function ago(epochSecs: number): string {
  const diff = Math.floor(Date.now() / 1000) - epochSecs
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  return `${Math.floor(diff / 86400)}d ago`
}

function Rows({ error, rows }: { error: Error | null; rows: AuditEntry[] }) {
  if (error)
    return <p className="text-danger text-sm">The audit log could not be read: {error.message}</p>
  if (rows.length === 0)
    return <p className="text-fg-muted text-sm">Nothing has been changed yet</p>
  return (
    <ul className="border-border divide-border divide-y rounded-lg border">
      {rows.map((e) => (
        <li className="flex items-baseline gap-3 px-4 py-2 text-sm" key={e.id}>
          <span className="text-fg shrink-0 font-medium">{e.action}</span>
          <span className="text-fg-secondary min-w-0 flex-1 truncate">{e.target}</span>
          <span className="text-fg-muted hidden shrink-0 sm:inline">{e.actor}</span>
          <time
            className="text-fg-muted shrink-0 text-xs tabular-nums"
            dateTime={new Date(e.timestamp * 1000).toISOString()}
          >
            {ago(e.timestamp)}
          </time>
        </li>
      ))}
    </ul>
  )
}
