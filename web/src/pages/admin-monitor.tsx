import type { WireOpsHistory, WireOpsState } from '@/wire/schemas/ops'

import { useQuery } from '@tanstack/react-query'

import { AlertsTable } from '@/components/admin-monitor/alerts-table'
import { CspPanel } from '@/components/admin-monitor/csp-panel'
import { DaemonUnreached, DaemonWarnings } from '@/components/admin-monitor/daemon-banners'
import { OwnPath, Transitions } from '@/components/admin-monitor/history-panel'
import { MaintenancePanel } from '@/components/admin-monitor/maintenance-panel'
import { SilencesPanel } from '@/components/admin-monitor/silences-panel'
import { AdminErrorState, AdminPageShell, AdminTableSkeleton } from '@/components/admin-page'
import { useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { wireOpsHistory, wireOpsState } from '@/wire/endpoints/ops'

type Query<T> = { data?: null | T; error: Error | null; isPending: boolean; refetch: () => unknown }

/**
 * What the ops daemon sees, through webapi: the alerts, the last day's
 * changes, its last completed check, the warnings about how far its
 * alerts can reach, and the silences and maintenance an administrator sets.
 * When webapi cannot see the daemon the page says so instead of showing
 * empty lists.
 */
export function AdminMonitor() {
  const t = useT()
  const state = useQuery({
    queryKey: adminKeys.opsState(),
    refetchInterval: 30_000,
    queryFn: ({ signal }) => wireOpsState(signal),
  })
  const reached = state.data?.reach === 'ok'
  const history = useQuery({
    enabled: reached,
    queryKey: adminKeys.opsHistory(),
    refetchInterval: 60_000,
    queryFn: ({ signal }) => wireOpsHistory(signal),
  })
  return (
    <AdminPageShell subtitle={t('admin.monitor.subtitle')} title={t('admin.monitor.title')}>
      <Body history={history} state={state} />
    </AdminPageShell>
  )
}

function Body({ history, state }: { history: Query<WireOpsHistory>; state: Query<WireOpsState> }) {
  const t = useT()
  if (state.error) return <AdminErrorState error={state.error} onRetry={() => state.refetch()} />
  const s = state.data
  if (!s) return <AdminTableSkeleton cols={4} />
  if (s.reach !== 'ok') return <DaemonUnreached state={s} />
  const daemon = s.state
  return (
    <div className="space-y-4">
      <DaemonWarnings state={s} />
      <p className="text-fg-secondary text-sm">{lastCycle(daemon.last_cycle, t)}</p>
      <AlertsTable alerts={daemon.alerts} />
      <HistoryBlock
        awaiting={daemon.notifications.own_path_awaiting_acceptance_250}
        history={history}
        rules={daemon.rules}
      />
      <CspPanel csp={daemon.csp} now={daemon.now} />
      <SilencesPanel state={daemon} />
      <MaintenancePanel state={daemon} />
    </div>
  )
}

function HistoryBlock({
  awaiting,
  history,
  rules,
}: {
  awaiting: string[]
  history: Query<WireOpsHistory>
  rules: Extract<WireOpsState, { reach: 'ok' }>['state']['rules']
}) {
  const t = useT()
  if (history.error) {
    return (
      <AdminErrorState
        error={history.error}
        onRetry={() => history.refetch()}
        what={t('admin.monitor.history')}
      />
    )
  }
  const h = history.data
  if (!h) return <AdminTableSkeleton cols={4} />
  return (
    <>
      <Transitions history={h} rules={rules} />
      <OwnPath awaiting={awaiting} history={h} />
    </>
  )
}

function lastCycle(
  last: null | { duration_ms: number; started_at: number },
  t: ReturnType<typeof useT>
): string {
  if (last === null) return t('admin.monitor.lastCycleNone')
  return t('admin.monitor.lastCycle', { at: formatFullDate(last.started_at), ms: last.duration_ms })
}
