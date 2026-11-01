import type { Column } from '@/components/admin-page'
import type { DmarcReport, DmarcSource } from '@/wire/schemas/dmarc'

import { useQuery } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { useSearchParams } from 'react-router'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { currentLocale, type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { fetchDmarcReports, fetchDmarcSources } from '@/wire/endpoints/dmarc'

const WINDOW_OPTIONS = [7, 30, 90] as const
const DEFAULT_DAYS = 30

const SOURCE_HEADERS: Key[] = [
  'admin.dmarc.sourceIp',
  'admin.mailAudit.messages',
  'admin.dmarc.passing',
  'admin.dmarc.passRate',
  'admin.domains.title',
]
const NO_REPORTS: DmarcReport[] = []

const sourceColumns = (t: ReturnType<typeof useT>): Column<DmarcSource>[] => [
  {
    header: t('admin.dmarc.sourceIp'),
    key: 'ip',
    cell: (s) => <span className="font-mono text-xs">{s.sourceIp}</span>,
  },
  { align: 'end', header: t('admin.mailAudit.messages'), key: 'total', cell: (s) => s.total },
  { align: 'end', header: t('admin.dmarc.passing'), key: 'passing', cell: (s) => s.passing },
  {
    align: 'end',
    header: t('admin.dmarc.passRate'),
    key: 'rate',
    cell: (s) => <PassRate passing={s.passing} total={s.total} />,
  },
  {
    header: t('admin.domains.title'),
    key: 'domains',
    cell: (s) => <span className="text-fg-muted text-xs">{s.domains.join(', ')}</span>,
  },
]

const reportColumns = (t: ReturnType<typeof useT>): Column<DmarcReport>[] => [
  { header: t('admin.dmarc.reporter'), key: 'org', cell: (r) => r.orgName },
  {
    header: t('admin.col.domain'),
    key: 'domain',
    cell: (r) => <span className="text-fg-muted text-xs">{r.policyDomain}</span>,
  },
  {
    align: 'end',
    header: t('admin.dmarc.window'),
    key: 'window',
    cell: (r) => (
      <span className="text-fg-muted text-xs whitespace-nowrap">
        {formatDay(r.begin)} – {formatDay(r.end)}
      </span>
    ),
  },
  {
    header: t('admin.dmarc.policy'),
    key: 'policy',
    cell: (r) => <span className="text-fg-muted text-xs">{r.policy}</span>,
  },
  { align: 'end', header: t('admin.mailAudit.messages'), key: 'total', cell: (r) => r.total },
  {
    align: 'end',
    header: t('admin.dmarc.passRate'),
    key: 'rate',
    cell: (r) => <PassRate passing={r.passing} total={r.total} />,
  },
]

export function AdminDmarc() {
  const t = useT()
  const [searchParams, setSearchParams] = useSearchParams()
  const days = parseDays(searchParams.get('days'))

  const setDays = (next: number) => {
    setSearchParams((prev) => {
      const params = new URLSearchParams(prev)
      if (next === DEFAULT_DAYS) params.delete('days')
      else params.set('days', String(next))
      return params
    })
  }

  const sourcesQuery = useQuery({
    queryKey: adminKeys.dmarcSources(days),
    queryFn: ({ signal }) => fetchDmarcSources(undefined, days, signal),
  })
  const reportsQuery = useQuery({
    queryKey: adminKeys.dmarcReports(),
    queryFn: ({ signal }) => fetchDmarcReports(50, signal),
  })
  const reports = reportsQuery.data ?? NO_REPORTS

  const windowPicker = (
    <div className="flex items-center gap-1">
      {WINDOW_OPTIONS.map((option) => (
        <button
          aria-pressed={option === days}
          className={windowClass(option === days)}
          key={option}
          onClick={() => setDays(option)}
          type="button"
        >
          {t('admin.dmarc.days', { n: option })}
        </button>
      ))}
    </div>
  )

  const renderSources = () => {
    if (sourcesQuery.isPending) {
      return (
        <AdminTableSkeleton
          cols={SOURCE_HEADERS.length}
          headers={SOURCE_HEADERS.map((h) => t(h))}
        />
      )
    }
    if (sourcesQuery.isError) {
      return (
        <AdminErrorState
          error={sourcesQuery.error}
          onRetry={() => void sourcesQuery.refetch()}
          what={t('admin.dmarc.sourcesName')}
        />
      )
    }
    const rollup = sourcesQuery.data
    return (
      <>
        <div className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
          <SummaryTile label={t('admin.dmarc.reported')} value={String(rollup.total)} />
          <SummaryTile label={t('admin.dmarc.passingDmarc')} value={String(rollup.passing)} />
          <SummaryTile
            label={t('admin.dmarc.passRate')}
            value={formatRate(passRate(rollup.total, rollup.passing))}
          />
          <SummaryTile label={t('admin.dmarc.reports')} value={String(rollup.reports)} />
        </div>
        <h3 className="text-fg mb-2 text-sm font-medium">{t('admin.dmarc.sources')}</h3>
        <p className="text-fg-muted mb-3 text-xs">{t('admin.dmarc.sourcesHelp', { days })}</p>
        <SourcesBody
          days={days}
          olderOnly={reports.length > 0 && rollup.reports === 0}
          sources={rollup.items}
        />
      </>
    )
  }

  const renderReports = () => {
    if (reportsQuery.isPending) {
      return <AdminTableSkeleton cols={reportColumns(t).length} rows={3} />
    }
    if (reportsQuery.isError) {
      return (
        <AdminErrorState
          error={reportsQuery.error}
          onRetry={() => void reportsQuery.refetch()}
          what={t('admin.dmarc.reportsName')}
        />
      )
    }
    if (reports.length === 0) {
      return (
        <AdminEmptyState
          description={t('admin.dmarc.noReportsHelp')}
          icon={<ShieldCheck className="h-10 w-10" />}
          title={t('admin.dmarc.noReports')}
        />
      )
    }
    return (
      <AdminTable
        columns={reportColumns(t)}
        label={t('admin.dmarc.reportsLabel')}
        rowKey={(r) => r.sid}
        rows={reports}
      />
    )
  }

  return (
    <AdminPageShell actions={windowPicker} subtitle={t('admin.dmarc.subtitle')} title="DMARC">
      <section className="mb-8">{renderSources()}</section>
      <section>
        <h3 className="text-fg mb-2 text-sm font-medium">{t('admin.dmarc.received')}</h3>
        {renderReports()}
      </section>
    </AdminPageShell>
  )
}

/** Unix seconds to a short local date. */
function formatDay(unixSeconds: number): string {
  if (unixSeconds === 0) return '—'
  return new Date(unixSeconds * 1000).toLocaleDateString(currentLocale(), {
    day: '2-digit',
    month: 'short',
  })
}

/** Null renders as an em dash, not "0%". */
function formatRate(rate: null | number): string {
  if (rate === null) return '—'
  return `${rate.toFixed(1)}%`
}

function parseDays(raw: null | string): number {
  const n = Number(raw)
  return WINDOW_OPTIONS.find((o) => o === n) ?? DEFAULT_DAYS
}

function PassRate({ passing, total }: { passing: number; total: number }) {
  const rate = passRate(total, passing)
  return <span className={rateClass(rate)}>{formatRate(rate)}</span>
}

/** Percentage of `passing` out of `total`, or null when nothing was seen. */
function passRate(total: number, passing: number): null | number {
  if (total === 0) return null
  return (passing / total) * 100
}

function rateClass(rate: null | number): string {
  if (rate === null) return 'text-fg-muted'
  if (rate >= 95) return 'text-success'
  if (rate >= 50) return 'text-warning'
  return 'text-danger'
}

function SourcesBody({
  days,
  olderOnly,
  sources,
}: {
  days: number
  /** reports are stored, but none began inside the window */
  olderOnly: boolean
  sources: DmarcSource[]
}) {
  const t = useT()
  if (sources.length > 0) {
    return (
      <AdminTable
        columns={sourceColumns(t)}
        label={t('admin.dmarc.sources')}
        rowKey={(s) => s.sourceIp}
        rows={sources}
      />
    )
  }
  if (olderOnly) {
    return (
      <AdminEmptyState
        description={t('admin.dmarc.olderOnlyHelp')}
        icon={<ShieldCheck className="h-10 w-10" />}
        title={t('admin.dmarc.noSourcesIn', { days })}
      />
    )
  }
  return (
    <AdminEmptyState
      description={t('admin.dmarc.noSourcesHelp')}
      icon={<ShieldCheck className="h-10 w-10" />}
      title={t('admin.dmarc.noSources')}
    />
  )
}

function SummaryTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="border-border bg-bg-secondary/50 rounded-lg border px-4 py-3">
      <p className="text-fg-muted text-xs">{label}</p>
      <p className="text-fg mt-1 text-lg font-semibold tabular-nums">{value}</p>
    </div>
  )
}

function windowClass(active: boolean): string {
  if (active) return 'rounded px-2 py-1 text-xs bg-accent text-white'
  return 'rounded px-2 py-1 text-xs text-fg-muted hover:bg-bg-secondary'
}
