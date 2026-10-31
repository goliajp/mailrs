import type { WireHashObservation, WireObservation } from '@/wire/schemas/screening'

import { useQuery } from '@tanstack/react-query'
import { ShieldAlert } from 'lucide-react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
} from '@/components/admin-page'
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { withdraw } from '@/lib/screening-withdraw'
import { HashForm, ObservationForm } from '@/pages/admin-screening-forms'
import { wireListScreening } from '@/wire/endpoints/screening'

const HEADERS = ['When', 'Source', 'Stance', 'Subject', 'Reason', 'By', '']

/**
 * The server's own observations: what the reputation judgement and the
 * attachment scan rest on. Each is a fact with its reason; a mistake is
 * withdrawn by a new observation, never deleted. The judgements run in
 * shadow until their replay report has been reviewed.
 */
export function AdminScreening() {
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: [...adminKeys.all(), 'screening'],
    queryFn: ({ signal }) => wireListScreening(signal),
  })
  const reload = () => void refetch()
  const withdrawn = new Set(
    [...(data?.observations ?? []), ...(data?.hashes ?? [])].flatMap((o) =>
      o.retracts ? [o.retracts] : []
    )
  )
  const columns = (isHash: boolean): Column<WireHashObservation | WireObservation>[] => [
    {
      header: 'When',
      key: 'at',
      cell: (r) => <span className="text-xs">{formatFullDate(r.at)}</span>,
    },
    {
      header: 'Source',
      key: 'source',
      cell: (r) => <span className="text-xs">{'source' in r ? r.source : 'hash'}</span>,
    },
    {
      header: 'Stance',
      key: 'stance',
      cell: (r) => <span className="font-mono text-xs">{r.stance}</span>,
    },
    {
      header: 'Subject',
      key: 'subject',
      cell: (r) => (
        <span className="font-mono text-xs break-all">
          {'value' in r ? `${r.kind} ${r.value}` : r.sha256}
          {r.retracts && ` → withdraws ${r.retracts}`}
        </span>
      ),
    },
    { header: 'Reason', key: 'reason', cell: (r) => <span className="text-xs">{r.reason}</span> },
    {
      header: 'By',
      key: 'by',
      cell: (r) => <span className="text-fg-muted text-xs">{r.actor ?? 'the server'}</span>,
    },
    {
      header: '',
      key: 'act',
      cell: (r) => {
        if (r.stance === 'retract' || withdrawn.has(r.id)) return null
        return (
          <button
            className="text-accent text-xs hover:underline"
            onClick={() => withdraw(r.id, isHash, reload)}
            type="button"
          >
            Withdraw
          </button>
        )
      },
    },
  ]
  const renderTable = (
    rows: (WireHashObservation | WireObservation)[],
    isHash: boolean,
    what: string
  ) => {
    if (isPending) return <AdminTableSkeleton cols={7} headers={HEADERS} rows={4} />
    if (error)
      return (
        <AdminErrorState error={error} onRetry={reload} retryDisabled={isFetching} what={what} />
      )
    if (rows.length === 0) {
      return (
        <AdminEmptyState
          description="Nothing recorded yet"
          icon={<ShieldAlert className="h-10 w-10" />}
          title={`No ${what}`}
        />
      )
    }
    return <AdminTable columns={columns(isHash)} label={what} rowKey={(r) => r.id} rows={rows} />
  }
  return (
    <AdminPageShell
      subtitle="What the reputation judgement and the attachment scan rest on. Both run in shadow for now: they are logged, not acted on."
      title="Screening"
    >
      <section className="space-y-3">
        <h3 className="text-sm font-semibold">Addresses and domains</h3>
        <ObservationForm onDone={reload} />
        {renderTable(data?.observations ?? [], false, 'observations')}
      </section>
      <section className="mt-8 space-y-3">
        <h3 className="text-sm font-semibold">Files</h3>
        <HashForm onDone={reload} />
        {renderTable(data?.hashes ?? [], true, 'file hashes')}
      </section>
    </AdminPageShell>
  )
}
