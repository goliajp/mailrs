import { useEffect, useState } from 'react'

import { Copyable } from '@/components/copy-button'
import { fetchJson } from '@/lib/api'

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

type SmtpConfig = {
  hostname: string
  smtp_port: number
  submission_port: number
  imap_port: number
  local_domains: string[]
  tls_enabled: boolean
}

function formatUptime(secs: number): string {
  const days = Math.floor(secs / 86400)
  const hours = Math.floor((secs % 86400) / 3600)
  const mins = Math.floor((secs % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h ${mins}m`
  if (hours > 0) return `${hours}h ${mins}m`
  return `${mins}m`
}

function StatCard({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-4">
      <p className="text-xs font-medium uppercase tracking-wider text-[var(--color-text-tertiary)]">{label}</p>
      <p className="mt-1 select-text text-2xl font-semibold text-[var(--color-text-primary)]">{value}</p>
      {sub && <p className="mt-0.5 select-text text-xs text-[var(--color-text-tertiary)]">{sub}</p>}
    </div>
  )
}

function StatusDot({ ok }: { ok: boolean }) {
  return (
    <span
      className={`inline-block h-2.5 w-2.5 rounded-full ${ok ? 'bg-[var(--color-status-success)]' : 'bg-[var(--color-status-danger)]'}`}
    />
  )
}

export function AdminOverview() {
  const [health, setHealth] = useState<HealthInfo | null>(null)
  const [smtp, setSmtp] = useState<SmtpConfig | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    fetchJson<HealthInfo>('/health').then(setHealth, () => setError('Failed to load health'))
    fetchJson<SmtpConfig>('/admin/config/smtp').then(setSmtp, () => {})
  }, [])

  if (error) {
    return (
      <div className="p-6 text-sm text-[var(--color-status-danger)]">{error}</div>
    )
  }

  return (
    <div className="h-full overflow-y-auto p-6">
      <h1 className="mb-6 text-lg font-semibold">Overview</h1>

      {health && (
        <div className="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-4">
          <StatCard
            label="Status"
            value={health.status}
            sub={`Level ${health.level}`}
          />
          <StatCard
            label="Uptime"
            value={formatUptime(health.uptime_secs)}
          />
          <StatCard
            label="Version"
            value={health.version}
          />
          <StatCard
            label="Services"
            value={`${[health.pg, health.valkey].filter(Boolean).length}/2`}
            sub={`PG ${health.pg ? 'OK' : 'DOWN'} / Valkey ${health.valkey ? 'OK' : 'DOWN'}`}
          />
          <StatCard
            label="Total Messages"
            value={health.total_messages.toLocaleString()}
          />
          <StatCard
            label="Active Sessions"
            value={health.active_sessions.toLocaleString()}
          />
          <StatCard
            label="Total Connections"
            value={health.total_connections.toLocaleString()}
          />
          <StatCard
            label="Cache Size"
            value={health.account_cache_size.toLocaleString()}
            sub="accounts cached"
          />
        </div>
      )}

      {/* service status */}
      {health && (
        <div className="mb-6">
          <h2 className="mb-3 text-sm font-medium text-[var(--color-text-tertiary)]">Services</h2>
          <div className="space-y-2">
            <div className="flex items-center gap-3 rounded-md border border-[var(--color-border-default)] px-4 py-3">
              <StatusDot ok={health.pg} />
              <span className="text-sm font-medium">PostgreSQL</span>
              <span className="ml-auto select-text text-xs text-[var(--color-text-tertiary)]">{health.pg ? 'Connected' : 'Unavailable'}</span>
            </div>
            <div className="flex items-center gap-3 rounded-md border border-[var(--color-border-default)] px-4 py-3">
              <StatusDot ok={health.valkey} />
              <span className="text-sm font-medium">Valkey / Redis</span>
              <span className="ml-auto select-text text-xs text-[var(--color-text-tertiary)]">{health.valkey ? 'Connected' : 'Unavailable'}</span>
            </div>
          </div>
        </div>
      )}

      {/* sessions and traffic */}
      {health && (
        <div className="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-2">
          <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-4">
            <h3 className="mb-3 text-sm font-medium text-[var(--color-text-tertiary)]">Sessions</h3>
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs text-[var(--color-text-tertiary)]">Active Sessions</span>
                <span className="font-semibold text-[var(--color-text-primary)]">{health.active_sessions.toLocaleString()}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-xs text-[var(--color-text-tertiary)]">Total Connections</span>
                <span className="font-semibold text-[var(--color-text-primary)]">{health.total_connections.toLocaleString()}</span>
              </div>
            </div>
          </div>

          <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-4">
            <h3 className="mb-3 text-sm font-medium text-[var(--color-text-tertiary)]">Traffic</h3>
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs text-[var(--color-text-tertiary)]">Total Messages</span>
                <span className="font-semibold text-[var(--color-text-primary)]">{health.total_messages.toLocaleString()}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-xs text-[var(--color-text-tertiary)]">Cached Accounts</span>
                <span className="font-semibold text-[var(--color-text-primary)]">{health.account_cache_size.toLocaleString()}</span>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* SMTP config */}
      {smtp && (
        <div>
          <h2 className="mb-3 text-sm font-medium text-[var(--color-text-tertiary)]">SMTP Configuration</h2>
          <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
            <table className="w-full text-sm">
              <tbody className="divide-y divide-[var(--color-border-default)]">
                <tr className="hover:bg-[var(--color-hover)]">
                  <td className="px-4 py-2.5 font-medium text-[var(--color-text-secondary)]">Hostname</td>
                  <td className="select-text px-4 py-2.5 font-mono text-[var(--color-text-primary)]"><Copyable value={smtp.hostname}>{smtp.hostname}</Copyable></td>
                </tr>
                <tr className="hover:bg-[var(--color-hover)]">
                  <td className="px-4 py-2.5 font-medium text-[var(--color-text-secondary)]">Ports</td>
                  <td className="select-text px-4 py-2.5 font-mono text-[var(--color-text-primary)]">
                    SMTP {smtp.smtp_port} / Submission {smtp.submission_port} / IMAP {smtp.imap_port}
                  </td>
                </tr>
                <tr className="hover:bg-[var(--color-hover)]">
                  <td className="px-4 py-2.5 font-medium text-[var(--color-text-secondary)]">TLS</td>
                  <td className="px-4 py-2.5 text-[var(--color-text-primary)]">
                    <span className={`inline-block rounded-md px-2 py-0.5 text-xs font-medium ${smtp.tls_enabled ? 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]' : 'bg-[var(--color-status-warning-subtle)] text-[var(--color-status-warning)]'}`}>
                      {smtp.tls_enabled ? 'Enabled' : 'Disabled'}
                    </span>
                  </td>
                </tr>
                <tr className="hover:bg-[var(--color-hover)]">
                  <td className="px-4 py-2.5 font-medium text-[var(--color-text-secondary)]">Local Domains</td>
                  <td className="px-4 py-2.5 text-[var(--color-text-primary)]">
                    <div className="flex flex-wrap gap-1.5">
                      {smtp.local_domains.map((d) => (
                        <Copyable key={d} value={d}>
                          <span className="rounded-md bg-[var(--color-bg-raised)] px-2 py-0.5 text-xs font-mono">
                            {d}
                          </span>
                        </Copyable>
                      ))}
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  )
}
