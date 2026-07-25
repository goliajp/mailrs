import type { DmarcReport, DmarcSource } from '@/wire/schemas/dmarc'

import { useQuery } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { ScrollableTable } from '@/components/scrollable-table'
import { adminKeys } from '@/lib/query-keys'
import { fetchDmarcReports, fetchDmarcSources } from '@/wire/endpoints/dmarc'

const WINDOW_OPTIONS = [7, 30, 90] as const

const SOURCE_HEADERS = ['Source IP', 'Messages', 'Passing', 'Alignment', 'Domains']
const REPORT_HEADERS = ['Reporter', 'Domain', 'Window', 'Policy', 'Messages', 'Alignment']

export function AdminDmarc() {
  const [days, setDays] = useState<number>(30)

  const sourcesQuery = useQuery({
    queryKey: adminKeys.dmarcSources(days),
    queryFn: ({ signal }) => fetchDmarcSources(undefined, days, signal),
  })
  const reportsQuery = useQuery({
    queryKey: adminKeys.dmarcReports(),
    queryFn: ({ signal }) => fetchDmarcReports(50, signal),
  })

  const windowPicker = (
    <div className="flex items-center gap-1">
      {WINDOW_OPTIONS.map((option) => {
        const classes = ['rounded px-2 py-1 text-xs']
        if (option === days) {
          classes.push('bg-accent text-white')
        } else {
          classes.push('text-fg-muted hover:bg-bg-secondary')
        }
        return (
          <button className={classes.join(' ')} key={option} onClick={() => setDays(option)}>
            {option}d
          </button>
        )
      })}
    </div>
  )

  if (sourcesQuery.isError) {
    return (
      <AdminPageShell title="DMARC">
        <AdminErrorState error={sourcesQuery.error} onRetry={() => void sourcesQuery.refetch()} />
      </AdminPageShell>
    )
  }

  if (sourcesQuery.isPending) {
    return (
      <AdminPageShell actions={windowPicker} title="DMARC">
        <AdminTableSkeleton cols={SOURCE_HEADERS.length} headers={SOURCE_HEADERS} />
      </AdminPageShell>
    )
  }

  const rollup = sourcesQuery.data
  const reports = reportsQuery.data ?? []
  const overallRate = alignmentRate(rollup.total, rollup.passing)

  return (
    <AdminPageShell actions={windowPicker} title="DMARC">
      <div className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <SummaryTile label="Messages seen" value={String(rollup.total)} />
        <SummaryTile label="Aligned" value={String(rollup.passing)} />
        <SummaryTile label="Alignment rate" value={formatRate(overallRate)} />
        <SummaryTile label="Reports" value={String(rollup.reports)} />
      </div>

      <section className="mb-8">
        <h3 className="text-fg mb-2 text-sm font-medium">Sending sources</h3>
        <p className="text-fg-muted mb-3 text-xs">
          Every IP that sent mail claiming one of your domains, over the last {days} days. A source
          at 0% alignment is either a misconfigured sender of yours or someone spoofing the domain.
        </p>
        {rollup.items.length === 0 && (
          <AdminEmptyState
            description="Aggregate reports arrive daily once receivers see mail from your domains. If you just repointed rua=, allow 24-48 hours."
            icon={<ShieldCheck className="h-10 w-10" />}
            title="No sending sources reported yet"
          />
        )}
        {rollup.items.length > 0 && <SourcesTable sources={rollup.items} />}
      </section>

      <section>
        <h3 className="text-fg mb-2 text-sm font-medium">Reports received</h3>
        {reports.length === 0 && (
          <AdminEmptyState
            description="Reports are stored as they arrive at the collector mailbox."
            icon={<ShieldCheck className="h-10 w-10" />}
            title="No reports stored yet"
          />
        )}
        {reports.length > 0 && <ReportsTable reports={reports} />}
      </section>
    </AdminPageShell>
  )
}

function AlignmentCell({ passing, total }: { passing: number; total: number }) {
  const rate = alignmentRate(total, passing)
  return <span className={rateClass(rate)}>{formatRate(rate)}</span>
}

/** Percentage of `passing` out of `total`, or null when nothing was seen. */
function alignmentRate(total: number, passing: number): null | number {
  if (total === 0) return null
  return (passing / total) * 100
}

/** Unix seconds to a short local date. */
function formatDay(unixSeconds: number): string {
  if (unixSeconds === 0) return '—'
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    day: '2-digit',
    month: 'short',
  })
}

/** Format a rate for display. Null renders as an em dash, not "0%". */
function formatRate(rate: null | number): string {
  if (rate === null) return '—'
  return `${rate.toFixed(1)}%`
}

/**
 * Tailwind text colour for an alignment rate. A source at 100% is
 * almost certainly ours and correctly configured; a source at 0% is
 * either a misconfigured sender of ours or someone spoofing the domain.
 */
function rateClass(rate: null | number): string {
  if (rate === null) return 'text-fg-muted'
  if (rate >= 95) return 'text-green-600 dark:text-green-400'
  if (rate >= 50) return 'text-amber-600 dark:text-amber-400'
  return 'text-red-600 dark:text-red-400'
}

function ReportsTable({ reports }: { reports: DmarcReport[] }) {
  return (
    <ScrollableTable>
      <table className="w-full text-left text-sm">
        <thead className="border-border bg-bg-secondary border-b">
          <tr>
            {REPORT_HEADERS.map((h) => (
              <th className="text-fg-muted px-4 py-2 text-xs font-medium" key={h}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {reports.map((r) => (
            <tr className="border-border/50 border-b last:border-0" key={r.sid}>
              <td className="text-fg px-4 py-2">{r.orgName}</td>
              <td className="text-fg-muted px-4 py-2 text-xs">{r.policyDomain}</td>
              <td className="text-fg-muted px-4 py-2 text-xs whitespace-nowrap">
                {formatDay(r.begin)} – {formatDay(r.end)}
              </td>
              <td className="text-fg-muted px-4 py-2 text-xs">{r.policy}</td>
              <td className="text-fg px-4 py-2 tabular-nums">{r.total}</td>
              <td className="px-4 py-2 tabular-nums">
                <AlignmentCell passing={r.passing} total={r.total} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollableTable>
  )
}

function SourcesTable({ sources }: { sources: DmarcSource[] }) {
  return (
    <ScrollableTable>
      <table className="w-full text-left text-sm">
        <thead className="border-border bg-bg-secondary border-b">
          <tr>
            {SOURCE_HEADERS.map((h) => (
              <th className="text-fg-muted px-4 py-2 text-xs font-medium" key={h}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sources.map((s) => (
            <tr className="border-border/50 border-b last:border-0" key={s.sourceIp}>
              <td className="text-fg px-4 py-2 font-mono text-xs">{s.sourceIp}</td>
              <td className="text-fg px-4 py-2 tabular-nums">{s.total}</td>
              <td className="text-fg px-4 py-2 tabular-nums">{s.passing}</td>
              <td className="px-4 py-2 tabular-nums">
                <AlignmentCell passing={s.passing} total={s.total} />
              </td>
              <td className="text-fg-muted px-4 py-2 text-xs">{s.domains.join(', ')}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollableTable>
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
