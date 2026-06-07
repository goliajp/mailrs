import { useQuery } from '@tanstack/react-query'

import { AdminErrorState, AdminPageShell } from '@/components/admin-page'
import { ScrollableTable } from '@/components/scrollable-table'
import { fetchJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'
import { type HealthInfo, HealthInfoSchema } from '@/lib/schemas'

// --- types ---

type AuditEntry = {
  action: string
  actor: string
  detail: string
  id: number
  target: string
  timestamp: number
}

type SmtpConfig = {
  hostname: string
  imap_port: number
  local_domains: string[]
  max_message_size?: number
  smtp_port: number
  submission_port: number
  tls_enabled: boolean
}

type StatusInfo = {
  active_connections: number
  queue?: {
    bounced: number
    delivered: number
    failed: number
    inflight: number
    pending: number
  }
  total_connections: number
  total_messages: number
  uptime_secs: number
}

// --- helpers ---

export function AdminOverview() {
  const {
    data: health = null,
    error: healthError,
    refetch: refetchHealth,
  } = useQuery({
    queryKey: adminKeys.overviewHealth(),
    refetchInterval: 10_000,
    queryFn: ({ signal }) => fetchJson<HealthInfo>('/health', signal, HealthInfoSchema.parse),
  })
  const {
    data: status = null,
    error: statusError,
    refetch: refetchStatus,
  } = useQuery({
    queryKey: adminKeys.overviewStatus(),
    refetchInterval: 10_000,
    queryFn: ({ signal }) => fetchJson<StatusInfo>('/status', signal),
  })
  const { data: smtp = null } = useQuery({
    queryKey: adminKeys.overviewSmtp(),
    refetchInterval: 10_000,
    queryFn: ({ signal }) => fetchJson<SmtpConfig>('/admin/config/smtp', signal),
  })
  const { data: audit = [] } = useQuery({
    queryKey: adminKeys.overviewAuditLog(),
    refetchInterval: 10_000,
    queryFn: ({ signal }) => fetchJson<AuditEntry[]>('/admin/audit-log?limit=10', signal),
  })

  if (healthError || statusError) {
    return (
      <AdminPageShell title="Overview">
        <AdminErrorState
          error={healthError ?? statusError ?? new Error('Failed to load')}
          onRetry={() => {
            refetchHealth()
            refetchStatus()
          }}
        />
      </AdminPageShell>
    )
  }

  const activeConns = status?.active_connections ?? health?.total_connections ?? 0
  const totalMsgs = status?.total_messages ?? health?.total_messages ?? 0
  const queuePending = status?.queue ? status.queue.pending + status.queue.inflight : 0
  const queueFailed = status?.queue?.failed ?? 0
  const activeSessions = health?.active_sessions ?? 0

  return (
    <AdminPageShell title="Overview">
      {/* status banner — fixed-height shell so the layout below doesn't
          jump when /api/health resolves */}
      <div className="mb-6 min-h-[60px]">
        {health ? <StatusBanner health={health} /> : <BannerSkeleton />}
      </div>

      {/* key metrics — values appear in-place; tabular-nums + min-height
          on the value line keeps the cards stable */}
      <div className="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-4">
        <MetricCard
          label="Active Connections"
          loading={!health && !status}
          sub={`${formatNumber(health?.total_connections ?? 0)} total`}
          value={formatNumber(activeConns)}
        />
        <MetricCard
          label="Total Messages"
          loading={!health && !status}
          value={formatNumber(totalMsgs)}
        />
        <MetricCard
          label="Queue Pending"
          loading={!status}
          sub={queueFailed > 0 ? `${formatNumber(queueFailed)} failed` : '0 failed'}
          value={formatNumber(queuePending)}
        />
        <MetricCard
          label="Active Users"
          loading={!health}
          sub="sessions"
          value={formatNumber(activeSessions)}
        />
      </div>

      {/* service health — reserved height so pills don't push later sections */}
      <div className="mb-6 min-h-[88px]">
        <h3 className="text-fg-muted mb-3 text-sm font-medium">Services</h3>
        {health ? (
          <div className="flex flex-wrap gap-3">
            <ServicePill detail={health.pg ? 'up' : 'down'} name="PostgreSQL" ok={health.pg} />
            <ServicePill detail={health.kevy ? 'up' : 'down'} name="Kevy" ok={health.kevy} />
            <ServicePill
              detail={smtp ? `:${smtp.smtp_port}` : undefined}
              name="SMTP"
              ok={health.pg}
            />
            <ServicePill
              detail={smtp ? `:${smtp.imap_port}` : undefined}
              name="IMAP"
              ok={health.pg}
            />
          </div>
        ) : (
          <div className="flex flex-wrap gap-3">
            {Array.from({ length: 4 }).map((_, i) => (
              <div className="bg-border h-12 w-32 animate-pulse rounded-lg" key={i} />
            ))}
          </div>
        )}
      </div>

      {/* quick info: smtp config + audit log — reserve height before smtp loads */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {smtp ? (
          <SmtpConfigPanel config={smtp} />
        ) : (
          <PanelSkeleton rows={5} title="SMTP Configuration" />
        )}
        <AuditLogPanel entries={audit} />
      </div>
    </AdminPageShell>
  )
}

function AuditLogPanel({ entries }: { entries: AuditEntry[] }) {
  return (
    <div className="border-border bg-surface rounded-lg border p-4">
      <h3 className="text-fg-muted mb-3 text-sm font-medium">Recent Audit Log</h3>
      {entries.length === 0 ? (
        <p className="text-fg-muted text-sm">No entries</p>
      ) : (
        <ScrollableTable>
          <table className="w-full text-xs">
            <tbody>
              {entries.map((e) => {
                const ip = e.detail.replace(/^ip=/, '').trim()
                return (
                  <tr className="border-border border-b last:border-0" key={e.id}>
                    <td className="text-fg-muted px-3 py-2.5 whitespace-nowrap">
                      {formatRelativeTime(e.timestamp)}
                    </td>
                    <td className="px-3 py-2.5 whitespace-nowrap">
                      <span
                        className={`rounded px-1.5 py-0.5 font-medium ${
                          e.action === 'login_failed'
                            ? 'bg-danger/10 text-danger'
                            : e.action === 'login'
                              ? 'bg-success/10 text-success'
                              : 'bg-bg-secondary text-fg-secondary'
                        }`}
                      >
                        {e.action.replace('_', ' ')}
                      </span>
                    </td>
                    <td className="text-fg-muted px-3 py-2.5 text-right tabular-nums">
                      {ip || e.target}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </ScrollableTable>
      )}
    </div>
  )
}

function BannerSkeleton() {
  return (
    <div className="border-border bg-surface flex items-center gap-4 rounded-lg border px-5 py-3">
      <div className="bg-border h-5 w-20 animate-pulse rounded-full" />
      <div className="bg-border h-4 w-12 animate-pulse rounded" />
      <div className="bg-border h-4 w-24 animate-pulse rounded" />
    </div>
  )
}

function formatNumber(n: number): string {
  return n.toLocaleString('en-US')
}

function formatRelativeTime(epochSecs: number): string {
  const now = Math.floor(Date.now() / 1000)
  const diff = now - epochSecs
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  return `${Math.floor(diff / 86400)}d ago`
}

// --- sub-components ---

function formatUptime(secs: number): string {
  const days = Math.floor(secs / 86400)
  const hours = Math.floor((secs % 86400) / 3600)
  const mins = Math.floor((secs % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h ${mins}m`
  if (hours > 0) return `${hours}h ${mins}m`
  return `${mins}m`
}

function MetricCard({
  label,
  loading,
  sub,
  value,
}: {
  label: string
  loading?: boolean
  sub?: string
  value: string
}) {
  return (
    <div className="border-border bg-surface rounded-lg border p-4">
      <p className="text-fg-muted text-sm">{label}</p>
      {/* fixed-height value row prevents reflow when the number arrives */}
      <div className="mt-1 h-8">
        {loading ? (
          <div className="bg-border h-7 w-16 animate-pulse rounded" />
        ) : (
          <p className="text-fg text-2xl font-bold tabular-nums">{value}</p>
        )}
      </div>
      <p className="text-fg-muted mt-1 min-h-[16px] text-xs">{sub ?? ''}</p>
    </div>
  )
}

function PanelSkeleton({ rows, title }: { rows: number; title: string }) {
  return (
    <div className="border-border bg-surface rounded-lg border p-4">
      <h3 className="text-fg-muted mb-3 text-sm font-medium">{title}</h3>
      <div className="space-y-2">
        {Array.from({ length: rows }).map((_, i) => (
          <div className="bg-border h-4 w-full animate-pulse rounded" key={i} />
        ))}
      </div>
    </div>
  )
}

function Row({ label, value }: { label: string; mono?: boolean; value: string }) {
  return (
    <div className="flex items-baseline gap-2 text-xs">
      <span className="text-fg-muted shrink-0">{label}</span>
      <span className="border-border min-w-0 flex-1 border-b border-dotted" />
      <span className="text-fg shrink-0 text-right">{value}</span>
    </div>
  )
}

function ServicePill({ detail, name, ok }: { detail?: string; name: string; ok: boolean }) {
  return (
    <div className="border-border bg-surface flex items-center gap-2 rounded-lg border px-4 py-3">
      <span className={`h-2.5 w-2.5 rounded-full ${ok ? 'bg-success' : 'bg-danger'}`} />
      <span className="text-fg text-sm font-medium">{name}</span>
      {detail && <span className="text-fg-muted text-xs">{detail}</span>}
    </div>
  )
}

function SmtpConfigPanel({ config }: { config: SmtpConfig }) {
  return (
    <div className="border-border bg-surface rounded-lg border p-4">
      <h3 className="text-fg-muted mb-3 text-sm font-medium">SMTP Configuration</h3>
      <div className="space-y-2">
        <Row label="Hostname" mono value={config.hostname} />
        <Row
          label="Ports"
          mono
          value={`SMTP :${config.smtp_port} / Submission :${config.submission_port} / IMAP :${config.imap_port}`}
        />
        <Row label="Domains" mono value={config.local_domains.join(', ')} />
        <Row label="TLS" value={config.tls_enabled ? 'Enabled' : 'Disabled'} />
        {config.max_message_size != null && (
          <Row label="Max Size" value={`${Math.round(config.max_message_size / 1024 / 1024)}MB`} />
        )}
      </div>
    </div>
  )
}

// --- main ---

function StatusBanner({ health }: { health: HealthInfo }) {
  const statusColor =
    health.status === 'healthy'
      ? 'bg-success/10 text-success'
      : health.status === 'degraded'
        ? 'bg-warning/10 text-warning'
        : 'bg-danger/10 text-danger'

  const dotColor =
    health.status === 'healthy'
      ? 'bg-success'
      : health.status === 'degraded'
        ? 'bg-warning'
        : 'bg-danger'

  return (
    <div className="border-border bg-surface flex items-center gap-4 rounded-lg border px-5 py-3">
      <span
        className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${statusColor}`}
      >
        <span className={`h-2 w-2 rounded-full ${dotColor}`} />
        {health.status.charAt(0).toUpperCase() + health.status.slice(1)}
      </span>
      <span className="text-fg-muted text-sm">v{health.version}</span>
      <span className="text-fg-muted text-sm">Uptime {formatUptime(health.uptime_secs)}</span>
    </div>
  )
}
