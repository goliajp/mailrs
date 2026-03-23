import { useCallback, useEffect, useState } from 'react'

import { fetchJson } from '@/lib/api'

// --- types ---

type HealthInfo = {
  status: string
  level: number
  pg: boolean
  valkey: boolean
  uptime_secs: number
  version: string
  active_sessions: number
  account_cache_size: number
  total_connections: number
  total_messages: number
}

type StatusInfo = {
  uptime_secs: number
  active_connections: number
  total_connections: number
  total_messages: number
  queue?: {
    pending: number
    inflight: number
    delivered: number
    failed: number
    bounced: number
  }
}

type SmtpConfig = {
  hostname: string
  smtp_port: number
  submission_port: number
  imap_port: number
  local_domains: string[]
  max_message_size?: number
  tls_enabled: boolean
}

type AuditEntry = {
  id: number
  timestamp: number
  actor: string
  action: string
  target: string
  detail: string
}

// --- helpers ---

function formatUptime(secs: number): string {
  const days = Math.floor(secs / 86400)
  const hours = Math.floor((secs % 86400) / 3600)
  const mins = Math.floor((secs % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h ${mins}m`
  if (hours > 0) return `${hours}h ${mins}m`
  return `${mins}m`
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

function StatusBanner({ health }: { health: HealthInfo }) {
  const statusColor =
    health.status === 'healthy'
      ? 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]'
      : health.status === 'degraded'
        ? 'bg-[var(--color-status-warning-subtle)] text-[var(--color-status-warning)]'
        : 'bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]'

  const dotColor =
    health.status === 'healthy'
      ? 'bg-[var(--color-status-success)]'
      : health.status === 'degraded'
        ? 'bg-[var(--color-status-warning)]'
        : 'bg-[var(--color-status-danger)]'

  return (
    <div className="flex items-center gap-4 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-5 py-3">
      <span
        className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${statusColor}`}
      >
        <span className={`h-2 w-2 rounded-full ${dotColor}`} />
        {health.status.charAt(0).toUpperCase() + health.status.slice(1)}
      </span>
      <span className="text-sm text-[var(--color-text-tertiary)]">v{health.version}</span>
      <span className="text-sm text-[var(--color-text-tertiary)]">
        Uptime {formatUptime(health.uptime_secs)}
      </span>
    </div>
  )
}

function MetricCard({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-4">
      <p className="text-sm text-[var(--color-text-tertiary)]">{label}</p>
      <p className="mt-1 text-2xl font-bold text-[var(--color-text-primary)]">{value}</p>
      {sub && <p className="mt-1 text-xs text-[var(--color-text-tertiary)]">{sub}</p>}
    </div>
  )
}

function ServicePill({ name, ok, detail }: { name: string; ok: boolean; detail?: string }) {
  return (
    <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-4 py-3">
      <span
        className={`h-2.5 w-2.5 rounded-full ${ok ? 'bg-[var(--color-status-success)]' : 'bg-[var(--color-status-danger)]'}`}
      />
      <span className="text-sm font-medium text-[var(--color-text-primary)]">{name}</span>
      {detail && <span className="text-xs text-[var(--color-text-tertiary)]">{detail}</span>}
    </div>
  )
}

function SmtpConfigPanel({ config }: { config: SmtpConfig }) {
  return (
    <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-4">
      <h3 className="mb-3 text-sm font-medium text-[var(--color-text-tertiary)]">
        SMTP Configuration
      </h3>
      <div className="space-y-2">
        <Row label="Hostname" value={config.hostname} mono />
        <Row
          label="Ports"
          value={`SMTP :${config.smtp_port} / Submission :${config.submission_port} / IMAP :${config.imap_port}`}
          mono
        />
        <Row label="Domains" value={config.local_domains.join(', ')} mono />
        <Row label="TLS" value={config.tls_enabled ? 'Enabled' : 'Disabled'} />
        {config.max_message_size != null && (
          <Row label="Max Size" value={`${Math.round(config.max_message_size / 1024 / 1024)}MB`} />
        )}
      </div>
    </div>
  )
}

function Row({ label, value }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline gap-2 text-xs">
      <span className="shrink-0 text-[var(--color-text-tertiary)]">{label}</span>
      <span className="min-w-0 flex-1 border-b border-dotted border-[var(--color-border-default)]" />
      <span className="shrink-0 text-right text-[var(--color-text-primary)]">{value}</span>
    </div>
  )
}

function AuditLogPanel({ entries }: { entries: AuditEntry[] }) {
  return (
    <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-4">
      <h3 className="mb-3 text-sm font-medium text-[var(--color-text-tertiary)]">
        Recent Audit Log
      </h3>
      {entries.length === 0 ? (
        <p className="text-sm text-[var(--color-text-tertiary)]">No entries</p>
      ) : (
        <table className="w-full text-xs">
          <tbody>
            {entries.map((e) => {
              const ip = e.detail.replace(/^ip=/, '').trim()
              return (
                <tr
                  key={e.id}
                  className="border-b border-[var(--color-border-default)] last:border-0"
                >
                  <td className="whitespace-nowrap py-1.5 pr-2 text-[var(--color-text-tertiary)]">
                    {formatRelativeTime(e.timestamp)}
                  </td>
                  <td className="whitespace-nowrap py-1.5 pr-2">
                    <span
                      className={`rounded px-1.5 py-0.5 font-medium ${
                        e.action === 'login_failed'
                          ? 'bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]'
                          : e.action === 'login'
                            ? 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]'
                            : 'bg-[var(--color-bg-sunken)] text-[var(--color-text-secondary)]'
                      }`}
                    >
                      {e.action.replace('_', ' ')}
                    </span>
                  </td>
                  <td className="py-1.5 text-right tabular-nums text-[var(--color-text-tertiary)]">
                    {ip || e.target}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      )}
    </div>
  )
}

// --- main ---

export function AdminOverview() {
  const [health, setHealth] = useState<HealthInfo | null>(null)
  const [status, setStatus] = useState<StatusInfo | null>(null)
  const [smtp, setSmtp] = useState<SmtpConfig | null>(null)
  const [audit, setAudit] = useState<AuditEntry[]>([])
  const [error, setError] = useState('')

  const refresh = useCallback(() => {
    fetchJson<HealthInfo>('/health').then(setHealth, () => setError('Failed to load health'))
    fetchJson<StatusInfo>('/status').then(setStatus, () => setError('Failed to load status'))
    fetchJson<SmtpConfig>('/admin/config/smtp').then(setSmtp, () => {})
    fetchJson<AuditEntry[]>('/admin/audit-log?limit=10').then(setAudit, () => {})
  }, [])

  useEffect(() => {
    refresh()
    const timer = setInterval(refresh, 10_000)
    return () => clearInterval(timer)
  }, [refresh])

  if (error) {
    return <div className="p-6 text-sm text-[var(--color-status-danger)]">{error}</div>
  }

  const activeConns = status?.active_connections ?? health?.total_connections ?? 0
  const totalMsgs = status?.total_messages ?? health?.total_messages ?? 0
  const queuePending = status?.queue ? status.queue.pending + status.queue.inflight : 0
  const queueFailed = status?.queue?.failed ?? 0
  const activeSessions = health?.active_sessions ?? 0

  return (
    <div className="h-full overflow-y-auto p-6">
      <h1 className="mb-6 text-lg font-semibold">Dashboard</h1>

      {/* status banner */}
      {health && (
        <div className="mb-6">
          <StatusBanner health={health} />
        </div>
      )}

      {/* key metrics */}
      <div className="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-4">
        <MetricCard
          label="Active Connections"
          value={formatNumber(activeConns)}
          sub={`${formatNumber(health?.total_connections ?? 0)} total`}
        />
        <MetricCard label="Total Messages" value={formatNumber(totalMsgs)} />
        <MetricCard
          label="Queue Pending"
          value={formatNumber(queuePending)}
          sub={queueFailed > 0 ? `${formatNumber(queueFailed)} failed` : '0 failed'}
        />
        <MetricCard label="Active Users" value={formatNumber(activeSessions)} sub="sessions" />
      </div>

      {/* service health */}
      {health && (
        <div className="mb-6">
          <h2 className="mb-3 text-sm font-medium text-[var(--color-text-tertiary)]">Services</h2>
          <div className="flex flex-wrap gap-3">
            <ServicePill name="PostgreSQL" ok={health.pg} detail={health.pg ? 'up' : 'down'} />
            <ServicePill name="Valkey" ok={health.valkey} detail={health.valkey ? 'up' : 'down'} />
            <ServicePill
              name="SMTP"
              ok={health.pg}
              detail={smtp ? `:${smtp.smtp_port}` : undefined}
            />
            <ServicePill
              name="IMAP"
              ok={health.pg}
              detail={smtp ? `:${smtp.imap_port}` : undefined}
            />
          </div>
        </div>
      )}

      {/* quick info: smtp config + audit log */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {smtp && <SmtpConfigPanel config={smtp} />}
        <AuditLogPanel entries={audit} />
      </div>
    </div>
  )
}
