import type { Key } from '@/i18n'
import type { AdminPushStats, ChannelTotals } from '@/wire/schemas/admin-push-stats'

import { useQuery } from '@tanstack/react-query'

import { useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { wireAdminPushStats } from '@/wire/endpoints/admin-push-stats'

import { count } from './format'

const DAYS = 14

type Channel = 'apns' | 'unifiedpush' | 'web'

const CHANNELS: { key: Channel; label: Key }[] = [
  { key: 'apns', label: 'admin.push.apns' },
  { key: 'web', label: 'admin.push.web' },
  { key: 'unifiedpush', label: 'admin.push.unifiedpush' },
]

const COLUMNS: { key: keyof ChannelTotals; label: Key }[] = [
  { key: 'sent', label: 'admin.push.sent' },
  { key: 'failed', label: 'admin.push.failed' },
  { key: 'suppressed_policy', label: 'admin.push.policy' },
  { key: 'suppressed_quiet', label: 'admin.push.quiet' },
]

/** Pushes per channel per day over the last two weeks, summed over every account. */
export function PushStats() {
  const t = useT()
  const stats = useQuery({
    queryKey: adminKeys.overviewPushStats(),
    refetchInterval: 60_000,
    queryFn: ({ signal }) => wireAdminPushStats(DAYS, signal),
  })
  return (
    <section aria-label={t('admin.push.title', { n: DAYS })}>
      <h3 className="text-fg mb-1 text-sm font-medium">{t('admin.push.title', { n: DAYS })}</h3>
      <Body error={stats.error} stats={stats.data ?? null} />
    </section>
  )
}

function Body({ error, stats }: { error: Error | null; stats: AdminPushStats | null }) {
  const t = useT()
  if (error)
    return (
      <p className="text-danger text-sm">{t('admin.push.unreadable', { reason: error.message })}</p>
    )
  if (stats === null)
    return (
      <div aria-busy className="bg-surface border-border h-24 animate-pulse rounded-lg border" />
    )
  return (
    <>
      <p className="text-fg-muted mb-2 text-xs">{t('admin.push.note', { n: stats.accounts })}</p>
      <div className="grid gap-4 xl:grid-cols-3">
        {CHANNELS.map((c) => (
          <ChannelTable channel={c.key} key={c.key} label={t(c.label)} stats={stats} />
        ))}
      </div>
    </>
  )
}

function ChannelTable({
  channel,
  label,
  stats,
}: {
  channel: Channel
  label: string
  stats: AdminPushStats
}) {
  const t = useT()
  const total = sum(stats.days.map((d) => d[channel]))
  return (
    <table className="border-border w-full overflow-hidden rounded-lg border text-sm">
      <caption className="text-fg-secondary px-1 pb-1 text-left text-xs font-medium">
        {label}
      </caption>
      <thead className="bg-surface text-fg-muted text-xs">
        <tr>
          <th className="px-3 py-1.5 text-left font-normal" scope="col">
            {t('admin.push.day')}
          </th>
          {COLUMNS.map((col) => (
            <th className="px-3 py-1.5 text-right font-normal" key={col.key} scope="col">
              {t(col.label)}
            </th>
          ))}
        </tr>
      </thead>
      <tbody className="divide-border divide-y">
        {stats.days.map((d) => (
          <tr key={d.day}>
            <th className="text-fg-secondary px-3 py-1 text-left font-normal" scope="row">
              {dayLabel(d.day)}
            </th>
            {COLUMNS.map((col) => (
              <td className="px-3 py-1 text-right tabular-nums" key={col.key}>
                {count(d[channel][col.key])}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
      <tfoot className="border-border border-t">
        <tr>
          <th className="text-fg px-3 py-1.5 text-left font-medium" scope="row">
            {t('admin.push.total')}
          </th>
          {COLUMNS.map((col) => (
            <td className="text-fg px-3 py-1.5 text-right font-medium tabular-nums" key={col.key}>
              {count(total[col.key])}
            </td>
          ))}
        </tr>
      </tfoot>
    </table>
  )
}

/** `20261107` as `11-07`; the days are UTC days. */
function dayLabel(day: string): string {
  return `${day.slice(4, 6)}-${day.slice(6, 8)}`
}

function sum(rows: ChannelTotals[]): ChannelTotals {
  const out: ChannelTotals = { failed: 0, sent: 0, suppressed_policy: 0, suppressed_quiet: 0 }
  for (const r of rows) {
    out.sent += r.sent
    out.failed += r.failed
    out.suppressed_policy += r.suppressed_policy
    out.suppressed_quiet += r.suppressed_quiet
  }
  return out
}
