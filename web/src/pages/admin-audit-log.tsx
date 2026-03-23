import { useCallback, useEffect, useState } from 'react'

import { fetchJson } from '@/lib/api'

type AuditEntry = {
  action: string
  actor: string
  detail: string
  id: number
  target: string
  timestamp: number
}

export function AdminAuditLog() {
  const [entries, setEntries] = useState<AuditEntry[]>([])

  const loadEntries = useCallback(async () => {
    try {
      const data = await fetchJson<AuditEntry[]>('/admin/audit-log?limit=200')
      setEntries(data)
    } catch {
      // keep current state
    }
  }, [])

  useEffect(() => {
    void loadEntries()
  }, [loadEntries])

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Audit Log</h2>
        <button
          className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
          onClick={loadEntries}
        >
          Refresh
        </button>
      </div>

      <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
            <tr>
              <th className="px-4 py-2.5 font-medium">Time</th>
              <th className="px-4 py-2.5 font-medium">Actor</th>
              <th className="px-4 py-2.5 font-medium">Action</th>
              <th className="px-4 py-2.5 font-medium">Target</th>
              <th className="px-4 py-2.5 font-medium">Detail</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr
                className="border-b border-[var(--color-border-default)] last:border-0"
                key={entry.id}
              >
                <td className="px-4 py-3 whitespace-nowrap text-[var(--color-text-secondary)]">
                  {formatTime(entry.timestamp)}
                </td>
                <td className="px-4 py-3 font-medium">{entry.actor}</td>
                <td
                  className={`px-4 py-3 font-medium ${actionColor(entry.action)}`}
                >
                  {entry.action}
                </td>
                <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                  {entry.target}
                </td>
                <td className="max-w-xs truncate px-4 py-3 text-[var(--color-text-tertiary)]">
                  {entry.detail}
                </td>
              </tr>
            ))}
            {entries.length === 0 && (
              <tr>
                <td
                  className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
                  colSpan={5}
                >
                  No audit log entries
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function actionColor(action: string): string {
  if (action === 'login_failed') return 'text-[var(--color-status-danger)]'
  if (action === 'login') return 'text-[var(--color-status-success)]'
  return 'text-[var(--color-text-primary)]'
}

function formatTime(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString()
}
