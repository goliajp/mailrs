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
import { adminKeys } from '@/lib/query-keys'
import { fetchDmarcReports, fetchDmarcSources } from '@/wire/endpoints/dmarc'

const WINDOW_OPTIONS = [7, 30, 90] as const
const DEFAULT_DAYS = 30

const SOURCE_HEADERS = ['Source IP', 'Messages', 'Passing', 'Pass rate', 'Domains']
const NO_REPORTS: DmarcReport[] = []

const SUBTITLE =
  'What mail providers report about mail sent as your domains. Providers send a daily aggregate report to the rua= address in each DMARC record; reports addressed to dmarc@ any domain here are stored as they arrive'

const sourceColumns: Column<DmarcSource>[] = [
  {
    header: 'Source IP',
    key: 'ip',
    cell: (s) => <span className="font-mono text-xs">{s.sourceIp}</span>,
  },
  { align: 'end', header: 'Messages', key: 'total', cell: (s) => s.total },
  { align: 'end', header: 'Passing', key: 'passing', cell: (s) => s.passing },
  {
    align: 'end',
    header: 'Pass rate',
    key: 'rate',
    cell: (s) => <PassRate passing={s.passing} total={s.total} />,
  },
  {
    header: 'Domains',
    key: 'domains',
    cell: (s) => <span className="text-fg-muted text-xs">{s.domains.join(', ')}</span>,
  },
]

const reportColumns: Column<DmarcReport>[] = [
  { header: 'Reporter', key: 'org', cell: (r) => r.orgName },
  {
    header: 'Domain',
    key: 'domain',
    cell: (r) => <span className="text-fg-muted text-xs">{r.policyDomain}</span>,
  },
  {
    align: 'end',
    header: 'Window',
    key: 'window',
    cell: (r) => (
      <span className="text-fg-muted text-xs whitespace-nowrap">
        {formatDay(r.begin)} – {formatDay(r.end)}
      </span>
    ),
  },
  {
    header: 'Policy',
    key: 'policy',
    cell: (r) => <span className="text-fg-muted text-xs">{r.policy}</span>,
  },
  { align: 'end', header: 'Messages', key: 'total', cell: (r) => r.total },
  {
    align: 'end',
    header: 'Pass rate',
    key: 'rate',
    cell: (r) => <PassRate passing={r.passing} total={r.total} />,
  },
]

export function AdminDmarc() {
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
          {option}d
        </button>
      ))}
    </div>
  )

  const renderSources = () => {
    if (sourcesQuery.isPending) {
      return <AdminTableSkeleton cols={SOURCE_HEADERS.length} headers={SOURCE_HEADERS} />
    }
    if (sourcesQuery.isError) {
      return (
        <AdminErrorState
          error={sourcesQuery.error}
          onRetry={() => void sourcesQuery.refetch()}
          what="the sending sources"
        />
      )
    }
    const rollup = sourcesQuery.data
    return (
      <>
        <div className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
          <SummaryTile label="Messages reported" value={String(rollup.total)} />
          <SummaryTile label="Passing DMARC" value={String(rollup.passing)} />
          <SummaryTile
            label="Pass rate"
            value={formatRate(passRate(rollup.total, rollup.passing))}
          />
          <SummaryTile label="Reports" value={String(rollup.reports)} />
        </div>
        <h3 className="text-fg mb-2 text-sm font-medium">Sending sources</h3>
        <p className="text-fg-muted mb-3 text-xs">
          Every IP that providers saw sending mail as one of your domains, in reports whose window
          began in the last {days} days. A source passing 0% is either a sender of yours whose SPF
          or DKIM does not align with the domain, or someone spoofing it
        </p>
        <SourcesBody
          days={days}
          olderOnly={reports.length > 0 && rollup.reports === 0}
          sources={rollup.items}
        />
      </>
    )
  }

  const renderReports = () => {
    if (reportsQuery.isPending) return <AdminTableSkeleton cols={reportColumns.length} rows={3} />
    if (reportsQuery.isError) {
      return (
        <AdminErrorState
          error={reportsQuery.error}
          onRetry={() => void reportsQuery.refetch()}
          what="the report list"
        />
      )
    }
    if (reports.length === 0) {
      return (
        <AdminEmptyState
          description="Set rua=mailto:dmarc@ one of your domains in each domain's DMARC record. Providers usually send their first report within a day or two"
          icon={<ShieldCheck className="h-10 w-10" />}
          title="No DMARC reports yet"
        />
      )
    }
    return (
      <AdminTable
        columns={reportColumns}
        label="DMARC reports"
        rowKey={(r) => r.sid}
        rows={reports}
      />
    )
  }

  return (
    <AdminPageShell actions={windowPicker} subtitle={SUBTITLE} title="DMARC">
      <section className="mb-8">{renderSources()}</section>
      <section>
        <h3 className="text-fg mb-2 text-sm font-medium">Reports received</h3>
        {renderReports()}
      </section>
    </AdminPageShell>
  )
}

/** Unix seconds to a short local date. */
function formatDay(unixSeconds: number): string {
  if (unixSeconds === 0) return '—'
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
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
  if (sources.length > 0) {
    return (
      <AdminTable
        columns={sourceColumns}
        label="Sending sources"
        rowKey={(s) => s.sourceIp}
        rows={sources}
      />
    )
  }
  if (olderOnly) {
    return (
      <AdminEmptyState
        description="Every stored report began before this window. Choose a longer window to include them"
        icon={<ShieldCheck className="h-10 w-10" />}
        title={`No sending sources in the last ${days} days`}
      />
    )
  }
  return (
    <AdminEmptyState
      description="Sources appear once the first aggregate report arrives. Each report lists the IPs that sent mail as your domain and whether that mail passed DMARC"
      icon={<ShieldCheck className="h-10 w-10" />}
      title="No sending sources yet"
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
