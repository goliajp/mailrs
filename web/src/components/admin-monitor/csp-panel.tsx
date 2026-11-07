import type { WireOpsCsp, WireOpsCspEntry, WireOpsCspTotals } from '@/wire/schemas/ops'

import { AdminSection, AdminTable, type Column } from '@/components/admin-page'
import { useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'

type T = ReturnType<typeof useT>

const WEEK_SECS = 7 * 86_400
const CLIENTS = ['desktop', 'mobile', 'android-webview', 'unknown'] as const

type Row = { key: string; label: string; total: number; week: number }

/**
 * The content security policy reports webapi counted, from the ops
 * daemon: the last seven days and since counting began, by directive, each
 * distinct report, and the pages served by client class. The policy is
 * enforced only after seven full days with no report.
 */
export function CspPanel({ csp, now }: { csp: WireOpsCsp; now: number }) {
  const t = useT()
  return (
    <AdminSection title={t('admin.monitor.csp.title')}>
      <CspBody csp={csp} now={now} t={t} />
    </AdminSection>
  )
}

function clientLabel(c: string, t: T): string {
  if (c === 'desktop') return t('admin.monitor.csp.client.desktop')
  if (c === 'mobile') return t('admin.monitor.csp.client.mobile')
  if (c === 'android-webview') return t('admin.monitor.csp.client.androidWebview')
  if (c === 'unknown') return t('admin.monitor.csp.client.unknown')
  return c
}

function countColumns(t: T): Column<Row>[] {
  return [
    { header: t('admin.monitor.csp.col.what'), key: 'what', cell: (r) => r.label },
    {
      align: 'end',
      header: t('admin.monitor.csp.col.week'),
      key: 'week',
      cell: (r) => r.week.toLocaleString(),
    },
    {
      align: 'end',
      header: t('admin.monitor.csp.col.total'),
      key: 'total',
      cell: (r) => r.total.toLocaleString(),
    },
  ]
}

/**
 * Report totals, then each directive, then pages served per client class.
 * A name missing from one window was counted there as zero: both windows
 * come from the same heartbeats.
 */
function countRows(week: WireOpsCspTotals, all: WireOpsCspTotals, t: T): Row[] {
  const rows: Row[] = [
    {
      key: 'reports',
      label: t('admin.monitor.csp.reports'),
      total: all.reports,
      week: week.reports,
    },
  ]
  for (const d of Object.keys(all.by_directive).sort()) {
    rows.push({
      key: `directive/${d}`,
      label: t('admin.monitor.csp.directive', { name: d }),
      total: all.by_directive[d] ?? 0,
      week: week.by_directive[d] ?? 0,
    })
  }
  const clients = new Set<string>([...CLIENTS, ...Object.keys(all.pages_served)])
  for (const c of clients) {
    rows.push({
      key: `pages/${c}`,
      label: t('admin.monitor.csp.pages', { client: clientLabel(c, t) }),
      total: all.pages_served[c] ?? 0,
      week: week.pages_served[c] ?? 0,
    })
  }
  return rows
}

function CspBody({ csp, now, t }: { csp: WireOpsCsp; now: number; t: T }) {
  const week = csp.last_7_days
  const all = csp.since_counting
  if (csp.counting_since === null || week === null || all === null) {
    return <p className="text-fg-muted p-4 text-sm">{t('admin.monitor.csp.notStarted')}</p>
  }
  return (
    <div className="space-y-3 pb-3">
      <p className="text-fg-secondary px-4 pt-3 text-sm">{sinceText(csp.counting_since, now, t)}</p>
      <AdminTable
        columns={countColumns(t)}
        label={t('admin.monitor.csp.counts')}
        rowKey={(r) => r.key}
        rows={countRows(week, all, t)}
      />
      <Distinct t={t} week={week} />
    </div>
  )
}

/** A field the browser left empty is not known, so it is not shown as blank. */
function dash(s: string): string {
  if (s === '') return '—'
  return s
}

function Distinct({ t, week }: { t: T; week: WireOpsCspTotals }) {
  const columns: Column<WireOpsCspEntry>[] = [
    { header: t('admin.monitor.csp.col.directive'), key: 'directive', cell: (e) => e.directive },
    {
      header: t('admin.monitor.csp.col.violated'),
      key: 'violated',
      cell: (e) => <span className="break-all">{dash(e.violated_directive)}</span>,
    },
    {
      header: t('admin.monitor.csp.col.blocked'),
      key: 'blocked',
      cell: (e) => <span className="break-all">{dash(e.blocked)}</span>,
    },
    { header: t('admin.monitor.csp.col.page'), key: 'page', cell: (e) => `/${e.page}` },
    {
      header: t('admin.monitor.csp.col.client'),
      key: 'client',
      cell: (e) => clientLabel(e.client, t),
    },
    { align: 'end', header: t('admin.monitor.csp.col.count'), key: 'count', cell: (e) => e.count },
    {
      align: 'end',
      header: t('admin.monitor.csp.col.first'),
      key: 'first',
      cell: (e) => formatFullDate(e.first_at),
    },
    {
      align: 'end',
      header: t('admin.monitor.csp.col.last'),
      key: 'last',
      cell: (e) => formatFullDate(e.last_at),
    },
  ]
  return (
    <div className="space-y-2">
      <h4 className="px-4 text-sm font-medium">{t('admin.monitor.csp.distinct')}</h4>
      {week.distinct.length === 0 && (
        <p className="text-fg-muted px-4 text-sm">{t('admin.monitor.csp.noneThisWeek')}</p>
      )}
      {week.distinct.length > 0 && (
        <AdminTable
          columns={columns}
          label={t('admin.monitor.csp.distinct')}
          rowKey={(e) =>
            [e.directive, e.violated_directive, e.blocked, e.page, e.client].join('\n')
          }
          rows={week.distinct}
        />
      )}
      {week.distinct_dropped > 0 && (
        <p className="text-fg-muted px-4 text-xs">
          {t('admin.monitor.csp.dropped', { n: week.distinct_dropped })}
        </p>
      )}
    </div>
  )
}

function sinceText(since: number, now: number, t: T): string {
  const at = formatFullDate(since)
  if (now - since < WEEK_SECS) return t('admin.monitor.csp.sinceShort', { at })
  return t('admin.monitor.csp.since', { at })
}
