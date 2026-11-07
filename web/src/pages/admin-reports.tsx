import type { WirePendingReport } from '@/wire/schemas/report'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTableSkeleton,
} from '@/components/admin-page'
import { type Key, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { fetchPendingReports, wireConfirmReport, wireDismissReport } from '@/wire/endpoints/report'

const EMPTY: WirePendingReport[] = []

/** Why a report waits, as the administrator reads it. */
const WAIT_LABEL: Record<string, Key> = {
  allow_listed: 'admin.reports.wait.allowListed',
  long_history: 'admin.reports.wait.longHistory',
  many_inboxes: 'admin.reports.wait.manyInboxes',
  no_ground: 'admin.reports.wait.noGround',
}

/**
 * Reports whose other copies wait: the reported message, how many inboxes
 * hold it, its sender's domain and why it waits, with up to ten of the
 * copies, so a confirmation is not made blind. Confirming takes them back
 * into Review as one batch that can be put back whole; dismissing leaves
 * them.
 */
export function AdminReports() {
  const t = useT()
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: [...adminKeys.all(), 'reports-pending'],
    queryFn: ({ signal }) => fetchPendingReports(signal),
  })
  const items = data ?? EMPTY
  const reload = () => void refetch()
  const decide = async (id: string, confirm: boolean) => {
    try {
      const o = await waitFor(confirm, id)
      toast.success(decidedText(confirm, o.taken_back, t))
    } catch {
      toast.error(t('admin.reports.failed'))
    } finally {
      reload()
    }
  }
  return (
    <AdminPageShell subtitle={t('admin.reports.description')} title={t('admin.reports.title')}>
      {isPending && <AdminTableSkeleton cols={3} />}
      {error && (
        <AdminErrorState
          error={error}
          onRetry={reload}
          retryDisabled={isFetching}
          what={t('admin.reports.title')}
        />
      )}
      {!isPending && !error && items.length === 0 && (
        <AdminEmptyState title={t('admin.reports.empty')} />
      )}
      <ul className="flex flex-col gap-3">
        {items.map((r) => (
          <li className="border-border rounded-md border p-3" key={r.outcome.report_id}>
            <p className="text-sm font-medium">{r.subject}</p>
            <p className="text-fg-muted text-xs">
              {r.from} · {r.sender_domain} · {formatFullDate(r.outcome.at)}
            </p>
            <p className="mt-1 text-xs">
              {t('admin.reports.summary', { count: r.outcome.inboxes, reporter: r.reporter })}
            </p>
            <p className="text-fg-muted flex flex-wrap gap-2 text-xs">
              {r.outcome.waits.map((w) => (
                <span className="bg-bg-secondary rounded px-1.5 py-0.5" key={w}>
                  {t(WAIT_LABEL[w] ?? 'admin.reports.wait.noGround')}
                </span>
              ))}
            </p>
            <ul className="text-fg-secondary mt-2 text-xs">
              {r.samples.map((s) => (
                <li key={s.user}>
                  {s.user}: {s.subject} ({s.from})
                </li>
              ))}
            </ul>
            <div className="mt-2 flex gap-2">
              <button
                className="bg-danger rounded px-2 py-1 text-xs text-white"
                onClick={() => void decide(r.outcome.report_id, true)}
                type="button"
              >
                {t('admin.reports.confirm')}
              </button>
              <button
                className="border-border rounded border px-2 py-1 text-xs"
                onClick={() => void decide(r.outcome.report_id, false)}
                type="button"
              >
                {t('admin.reports.dismiss')}
              </button>
            </div>
          </li>
        ))}
      </ul>
    </AdminPageShell>
  )
}

function decidedText(confirm: boolean, takenBack: number, t: ReturnType<typeof useT>): string {
  if (confirm) return t('admin.reports.confirmed', { count: takenBack })
  return t('admin.reports.dismissed')
}

function waitFor(confirm: boolean, id: string) {
  if (confirm) return wireConfirmReport(id)
  return wireDismissReport(id)
}
