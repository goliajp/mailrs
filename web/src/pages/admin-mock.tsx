import type { MockAdminNamespace, MockAdminOverview } from '@/wire/schemas/mock'

import { useQuery } from '@tanstack/react-query'
import { FlaskConical } from 'lucide-react'
import { useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { MobileModal } from '@/components/mobile-modal'
import { formatReceived } from '@/components/mock/format'
import { ScrollableTable } from '@/components/scrollable-table'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { formatSize } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { wireAdminMockOverview, wireDeleteMockNamespace } from '@/wire/endpoints/mock'

const HEADERS = [
  'Namespace',
  'Owner',
  'Messages',
  'Dropped',
  'Size',
  'SMTP',
  'Webhook',
  'Created',
  '',
]

/** Every user's mock namespaces and the store's totals; an admin can delete any. */
export function AdminMock() {
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
  const overview = useQuery({
    queryKey: adminKeys.mock(),
    refetchInterval: 30_000,
    queryFn: ({ signal }) => wireAdminMockOverview(signal),
  })
  const remove = useAdminMutation({
    invalidateKey: adminKeys.mock(),
    mutationFn: (ns: string) => wireDeleteMockNamespace(ns),
    successMsg: (ns) => `Namespace ${ns} deleted`,
  })

  return (
    <AdminPageShell
      subtitle="Disposable test mailboxes. Mail is kept 3 days, 500 messages per namespace"
      title="Mock Mail"
    >
      <Body
        data={overview.data ?? null}
        error={overview.error}
        onDelete={setDeleteTarget}
        onRetry={() => void overview.refetch()}
        pending={overview.isPending}
      />
      {deleteTarget !== null && (
        <MobileModal onClose={() => setDeleteTarget(null)} open>
          <div className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg">
            <p className="text-fg-secondary mb-4 text-sm">
              Delete namespace <span className="text-fg font-mono">{deleteTarget}</span>, its
              messages, SMTP password and webhook? Its addresses stop receiving at once
            </p>
            <div className="flex justify-end gap-2">
              <button
                className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
                onClick={() => setDeleteTarget(null)}
                type="button"
              >
                Cancel
              </button>
              <button
                className="bg-danger rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
                disabled={remove.isPending}
                onClick={() =>
                  remove.mutate(deleteTarget, { onSettled: () => setDeleteTarget(null) })
                }
                type="button"
              >
                Delete
              </button>
            </div>
          </div>
        </MobileModal>
      )}
    </AdminPageShell>
  )
}

function Body({
  data,
  error,
  onDelete,
  onRetry,
  pending,
}: {
  data: MockAdminOverview | null
  error: Error | null
  onDelete: (ns: string) => void
  onRetry: () => void
  pending: boolean
}) {
  if (pending) return <AdminTableSkeleton cols={HEADERS.length} headers={HEADERS} />
  if (error) return <AdminErrorState error={error} onRetry={onRetry} />
  if (data === null) return null
  if (data.domain === '') {
    return (
      <AdminEmptyState
        description="Set MAILRS_MOCK_DOMAIN on the receiver, fastcore, webapi and sender to turn it on"
        icon={<FlaskConical className="h-10 w-10" />}
        title="Mock mail is not enabled on this deployment"
      />
    )
  }
  return (
    <div className="space-y-4">
      <Totals data={data} />
      <NamespaceTable namespaces={data.namespaces} onDelete={onDelete} />
    </div>
  )
}

function NamespaceTable({
  namespaces,
  onDelete,
}: {
  namespaces: MockAdminNamespace[]
  onDelete: (ns: string) => void
}) {
  if (namespaces.length === 0) {
    return (
      <AdminEmptyState
        description="Users create namespaces on the Mock Mail page"
        icon={<FlaskConical className="h-10 w-10" />}
        title="No namespaces yet"
      />
    )
  }
  return (
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
          {namespaces.map((n) => (
            <tr className="border-border border-b last:border-0" key={n.name}>
              <td className="px-4 py-3 font-mono text-xs">{n.name}</td>
              <td className="text-fg-secondary px-4 py-3 text-xs">{n.owner}</td>
              <td className="px-4 py-3">{n.messages}</td>
              <td className="px-4 py-3">{n.dropped}</td>
              <td className="text-fg-secondary px-4 py-3 text-xs">{formatSize(n.bytes)}</td>
              <td className="text-fg-secondary px-4 py-3 text-xs">{onOff(n.smtp_enabled)}</td>
              <td className="text-fg-secondary max-w-[200px] truncate px-4 py-3 text-xs">
                {n.webhook_url ?? '—'}
              </td>
              <td className="text-fg-muted px-4 py-3 text-xs">{formatReceived(n.created_at)}</td>
              <td className="px-4 py-3 text-right">
                <button
                  className="text-danger text-xs transition-colors hover:opacity-70"
                  onClick={() => onDelete(n.name)}
                  type="button"
                >
                  Delete
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollableTable>
  )
}

function onOff(on: boolean): string {
  if (on) return 'on'
  return '—'
}

function Totals({ data }: { data: MockAdminOverview }) {
  const share = Math.min(100, Math.round((data.bytes / data.max_bytes) * 100))
  const owners = new Set(data.namespaces.map((n) => n.owner)).size
  const tiles: { label: string; value: string }[] = [
    { label: 'Domain', value: data.domain },
    { label: 'Namespaces', value: `${data.namespaces.length} (${owners} owners)` },
    {
      label: 'Storage',
      value: `${formatSize(data.bytes)} of ${formatSize(data.max_bytes)} (${share}%)`,
    },
    { label: 'Received', value: String(data.received) },
    { label: 'Dropped over the cap', value: String(data.dropped) },
    { label: 'Refused, no such namespace', value: String(data.rejected_unknown_ns) },
  ]
  return (
    <dl className="grid grid-cols-2 gap-3 md:grid-cols-3">
      {tiles.map((t) => (
        <div className="border-border rounded-lg border px-3 py-2.5" key={t.label}>
          <dt className="text-fg-muted text-xs">{t.label}</dt>
          <dd className="text-fg mt-0.5 truncate text-sm font-medium">{t.value}</dd>
        </div>
      ))}
    </dl>
  )
}
