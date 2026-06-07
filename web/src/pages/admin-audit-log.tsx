import { Button } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { ScrollText } from 'lucide-react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
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

const HEADERS = ['Time', 'Actor', 'Action', 'Target', 'Detail']

export function AdminAuditLog() {
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.auditLog(),
    queryFn: ({ signal }) => fetchJson<AuditEntry[]>('/admin/audit-log?limit=200', signal),
  })
  const entries = data ?? []

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
      {isPending ? (
        <AdminTableSkeleton cols={5} headers={HEADERS} rows={6} />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} retryDisabled={isFetching} />
      ) : entries.length === 0 ? (
        <AdminEmptyState
          description="Actions taken by admins will appear here."
          icon={<ScrollText className="h-10 w-10" />}
          title="No audit log entries"
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
            </tbody>
          </table>
        </ScrollableTable>
      )}
    </AdminPageShell>
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
