import { useEffect, useState } from 'react'

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
    <div className="rounded-lg border border-zinc-200 bg-white p-4 dark:border-zinc-700 dark:bg-zinc-900">
      <p className="text-xs font-medium uppercase tracking-wider text-zinc-400">{label}</p>
      <p className="mt-1 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">{value}</p>
      {sub && <p className="mt-0.5 text-xs text-zinc-500">{sub}</p>}
    </div>
  )
}

function StatusDot({ ok }: { ok: boolean }) {
  return (
    <span
      className={`inline-block h-2.5 w-2.5 rounded-full ${ok ? 'bg-green-500' : 'bg-red-500'}`}
    />
  )
}

export function AdminOverview() {
  const [health, setHealth] = useState<HealthInfo | null>(null)
  const [smtp, setSmtp] = useState<SmtpConfig | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    fetchJson<HealthInfo>('/admin/health').then(setHealth, () => setError('Failed to load health'))
    fetchJson<SmtpConfig>('/admin/config/smtp').then(setSmtp, () => {})
  }, [])

  if (error) {
    return (
      <div className="p-6 text-sm text-red-500">{error}</div>
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
          <h2 className="mb-3 text-sm font-medium text-zinc-500">Services</h2>
          <div className="space-y-2">
            <div className="flex items-center gap-3 rounded-lg border border-zinc-200 px-4 py-3 dark:border-zinc-700">
              <StatusDot ok={health.pg} />
              <span className="text-sm font-medium">PostgreSQL</span>
              <span className="ml-auto text-xs text-zinc-400">{health.pg ? 'Connected' : 'Unavailable'}</span>
            </div>
            <div className="flex items-center gap-3 rounded-lg border border-zinc-200 px-4 py-3 dark:border-zinc-700">
              <StatusDot ok={health.valkey} />
              <span className="text-sm font-medium">Valkey / Redis</span>
              <span className="ml-auto text-xs text-zinc-400">{health.valkey ? 'Connected' : 'Unavailable'}</span>
            </div>
          </div>
        </div>
      )}

      {/* sessions and traffic */}
      {health && (
        <div className="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-2">
          <div className="rounded-lg border border-zinc-200 bg-white p-4 dark:border-zinc-700 dark:bg-zinc-900">
            <h3 className="mb-3 text-sm font-medium text-zinc-500">Sessions</h3>
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs text-zinc-400">Active Sessions</span>
                <span className="font-semibold text-zinc-900 dark:text-zinc-100">{health.active_sessions.toLocaleString()}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-xs text-zinc-400">Total Connections</span>
                <span className="font-semibold text-zinc-900 dark:text-zinc-100">{health.total_connections.toLocaleString()}</span>
              </div>
            </div>
          </div>

          <div className="rounded-lg border border-zinc-200 bg-white p-4 dark:border-zinc-700 dark:bg-zinc-900">
            <h3 className="mb-3 text-sm font-medium text-zinc-500">Traffic</h3>
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs text-zinc-400">Total Messages</span>
                <span className="font-semibold text-zinc-900 dark:text-zinc-100">{health.total_messages.toLocaleString()}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-xs text-zinc-400">Cached Accounts</span>
                <span className="font-semibold text-zinc-900 dark:text-zinc-100">{health.account_cache_size.toLocaleString()}</span>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* SMTP config */}
      {smtp && (
        <div>
          <h2 className="mb-3 text-sm font-medium text-zinc-500">SMTP Configuration</h2>
          <div className="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700">
            <table className="w-full text-sm">
              <tbody className="divide-y divide-zinc-200 dark:divide-zinc-700">
                <tr className="hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
                  <td className="px-4 py-2.5 font-medium text-zinc-600 dark:text-zinc-400">Hostname</td>
                  <td className="px-4 py-2.5 font-mono text-zinc-900 dark:text-zinc-100">{smtp.hostname}</td>
                </tr>
                <tr className="hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
                  <td className="px-4 py-2.5 font-medium text-zinc-600 dark:text-zinc-400">Ports</td>
                  <td className="px-4 py-2.5 font-mono text-zinc-900 dark:text-zinc-100">
                    SMTP {smtp.smtp_port} / Submission {smtp.submission_port} / IMAP {smtp.imap_port}
                  </td>
                </tr>
                <tr className="hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
                  <td className="px-4 py-2.5 font-medium text-zinc-600 dark:text-zinc-400">TLS</td>
                  <td className="px-4 py-2.5 text-zinc-900 dark:text-zinc-100">
                    <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${smtp.tls_enabled ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400' : 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400'}`}>
                      {smtp.tls_enabled ? 'Enabled' : 'Disabled'}
                    </span>
                  </td>
                </tr>
                <tr className="hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
                  <td className="px-4 py-2.5 font-medium text-zinc-600 dark:text-zinc-400">Local Domains</td>
                  <td className="px-4 py-2.5 text-zinc-900 dark:text-zinc-100">
                    <div className="flex flex-wrap gap-1.5">
                      {smtp.local_domains.map((d) => (
                        <span key={d} className="rounded bg-zinc-100 px-2 py-0.5 text-xs font-mono dark:bg-zinc-800">
                          {d}
                        </span>
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
