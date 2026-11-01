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
import { type Key, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { withdraw } from '@/lib/screening-withdraw'
import { HashForm, ObservationForm } from '@/pages/admin-screening-forms'
import { wireListScreening } from '@/wire/endpoints/screening'

const HEADERS: Key[] = [
  'admin.screening.when',
  'admin.aliases.source',
  'admin.screening.stance',
  'admin.screening.subject',
  'admin.screening.reason',
  'admin.greylist.by',
]

/**
 * The server's own observations: what the reputation judgement and the
 * attachment scan rest on. Each is a fact with its reason; a mistake is
 * withdrawn by a new observation, never deleted. The judgements run in
 * shadow until their replay report has been reviewed.
 */
export function AdminScreening() {
  const t = useT()
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: [...adminKeys.all(), 'screening'],
    queryFn: ({ signal }) => wireListScreening(signal),
  })
  const reload = () => void refetch()
  const withdrawn = new Set(
    [...(data?.observations ?? []), ...(data?.hashes ?? [])].flatMap((o) => withdrawsOf(o))
  )
  const columns = (isHash: boolean): Column<WireHashObservation | WireObservation>[] => [
    {
      header: t('admin.screening.when'),
      key: 'at',
      cell: (r) => <span className="text-xs">{formatFullDate(r.at)}</span>,
    },
    {
      header: t('admin.aliases.source'),
      key: 'source',
      cell: (r) => <span className="text-xs">{sourceOf(r)}</span>,
    },
    {
      header: t('admin.screening.stance'),
      key: 'stance',
      cell: (r) => <span className="font-mono text-xs">{r.stance}</span>,
    },
    {
      header: t('admin.screening.subject'),
      key: 'subject',
      cell: (r) => (
        <span className="font-mono text-xs break-all">
          {subjectOf(r)}
          {r.retracts && ` → ${t('admin.screening.withdraws', { id: r.retracts })}`}
        </span>
      ),
    },
    {
      header: t('admin.screening.reason'),
      key: 'reason',
      cell: (r) => <span className="text-xs">{r.reason}</span>,
    },
    {
      header: t('admin.greylist.by'),
      key: 'by',
      cell: (r) => (
        <span className="text-fg-muted text-xs">{r.actor ?? t('admin.screening.theServer')}</span>
      ),
    },
    {
      header: '',
      key: 'act',
      cell: (r) => {
        if (r.stance === 'retract' || withdrawn.has(r.id)) return null
        return (
          <button
            className="text-accent text-xs hover:underline"
            onClick={() => withdraw(r.id, isHash, reload, t)}
            type="button"
          >
            {t('admin.screening.withdraw')}
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
    if (isPending) {
      return <AdminTableSkeleton cols={7} headers={[...HEADERS.map((h) => t(h)), '']} rows={4} />
    }
    if (error)
      return (
        <AdminErrorState error={error} onRetry={reload} retryDisabled={isFetching} what={what} />
      )
    if (rows.length === 0) {
      return (
        <AdminEmptyState
          description={t('admin.screening.nothingYet')}
          icon={<ShieldAlert className="h-10 w-10" />}
          title={t('admin.screening.noneOf', { what })}
        />
      )
    }
    return <AdminTable columns={columns(isHash)} label={what} rowKey={(r) => r.id} rows={rows} />
  }
  return (
    <AdminPageShell subtitle={t('admin.screening.subtitle')} title={t('admin.screening.title')}>
      <section className="space-y-3">
        <h3 className="text-sm font-semibold">{t('admin.screening.addresses')}</h3>
        <ObservationForm onDone={reload} />
        {renderTable(data?.observations ?? [], false, t('admin.screening.observations'))}
      </section>
      <section className="mt-8 space-y-3">
        <h3 className="text-sm font-semibold">{t('admin.screening.files')}</h3>
        <HashForm onDone={reload} />
        {renderTable(data?.hashes ?? [], true, t('admin.screening.hashes'))}
      </section>
    </AdminPageShell>
  )
}

function sourceOf(r: WireHashObservation | WireObservation): string {
  if ('source' in r) return r.source
  return 'hash'
}

function subjectOf(r: WireHashObservation | WireObservation): string {
  if ('value' in r) return `${r.kind} ${r.value}`
  return r.sha256
}

function withdrawsOf(o: WireHashObservation | WireObservation): string[] {
  if (o.retracts) return [o.retracts]
  return []
}
