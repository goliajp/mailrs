import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { useT } from '@/i18n'
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
  const t = useT()
  const audit = useQuery({
    queryKey: adminKeys.overviewAuditLog(),
    refetchInterval: 30_000,
    queryFn: ({ signal }) => adminListGet<AuditEntry>('/admin/audit-log?limit=8', signal),
  })
  const rows = audit.data ?? EMPTY
  return (
    <section aria-label={t('admin.recent.title')}>
      <div className="mb-2 flex items-baseline justify-between">
        <h3 className="text-fg text-sm font-medium">{t('admin.recent.title')}</h3>
        <Link className="text-fg-muted hover:text-fg text-xs" to="/admin/audit-log">
          {t('admin.auditLog')}
        </Link>
      </div>
      <Rows error={audit.error} rows={rows} />
    </section>
  )
}

function ago(epochSecs: number, t: ReturnType<typeof useT>): string {
  const diff = Math.floor(Date.now() / 1000) - epochSecs
  if (diff < 60) return t('admin.ago.now')
  if (diff < 3600) return t('admin.ago.minutes', { n: Math.floor(diff / 60) })
  if (diff < 86400) return t('admin.ago.hours', { n: Math.floor(diff / 3600) })
  return t('admin.ago.days', { n: Math.floor(diff / 86400) })
}

function Rows({ error, rows }: { error: Error | null; rows: AuditEntry[] }) {
  const t = useT()
  if (error)
    return (
      <p className="text-danger text-sm">
        {t('admin.recent.unreadable', { reason: error.message })}
      </p>
    )
  if (rows.length === 0) return <p className="text-fg-muted text-sm">{t('admin.recent.empty')}</p>
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
            {ago(e.timestamp, t)}
          </time>
        </li>
      ))}
    </ul>
  )
}
