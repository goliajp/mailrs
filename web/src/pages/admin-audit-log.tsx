import { useQuery } from '@tanstack/react-query'

import { ScrollableTable } from '@/components/scrollable-table'
import { fetchJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

type AuditEntry = {
  action: string
  actor: string
  detail: string
  id: number
  target: string
  timestamp: number
}

export function AdminAuditLog() {
  const { data: entries = [], refetch } = useQuery({
    queryKey: adminKeys.auditLog(),
    queryFn: () => fetchJson<AuditEntry[]>('/admin/audit-log?limit=200'),
  })

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Audit Log</h2>
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:opacity-90"
          onClick={() => refetch()}
        >
          Refresh
        </button>
      </div>

      <ScrollableTable>
        <table className="w-full text-left text-sm">
          <thead className="border-border bg-bg-secondary border-b">
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
              <tr className="border-border border-b last:border-0" key={entry.id}>
                <td className="text-fg-secondary px-4 py-3 whitespace-nowrap">
                  {formatTime(entry.timestamp)}
                </td>
                <td className="px-4 py-3 font-medium">{entry.actor}</td>
                <td className={`px-4 py-3 font-medium ${actionColor(entry.action)}`}>
                  {entry.action}
                </td>
                <td className="text-fg-secondary px-4 py-3">{entry.target}</td>
                <td className="text-fg-muted max-w-xs truncate px-4 py-3">{entry.detail}</td>
              </tr>
            ))}
            {entries.length === 0 && (
              <tr>
                <td className="text-fg-muted px-4 py-8 text-center" colSpan={5}>
                  No audit log entries
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </ScrollableTable>
    </div>
  )
}

function actionColor(action: string): string {
  if (action === 'login_failed') return 'text-danger'
  if (action === 'login') return 'text-success'
  return 'text-fg'
}

function formatTime(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString()
}
