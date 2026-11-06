import type { WireOpsAlert } from '@/wire/schemas/ops'

import { useAtomValue } from 'jotai'

import { AdminEmptyState, AdminSection, AdminTable, type Column } from '@/components/admin-page'
import { langAtom, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'

import { phaseKey, severityKey, stepOf, titleOf } from './texts'

/** Every alert not in `ok`, with the first step in the interface language. */
export function AlertsTable({ alerts }: { alerts: readonly WireOpsAlert[] }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  const active = alerts.filter((a) => a.phase !== 'ok')
  const columns: Column<WireOpsAlert>[] = [
    {
      header: t('admin.monitor.col.alert'),
      key: 'alert',
      cell: (a) => <span className="font-medium">{titleOf(lang, a)}</span>,
    },
    {
      header: t('admin.monitor.col.state'),
      key: 'state',
      cell: (a) => phaseText(a.phase, t),
    },
    {
      header: t('admin.monitor.col.severity'),
      key: 'severity',
      cell: (a) => t(severityKey(a.severity)),
    },
    {
      header: t('admin.monitor.col.object'),
      key: 'object',
      cell: (a) => <span className="break-all">{a.object}</span>,
    },
    {
      align: 'end',
      header: t('admin.monitor.col.since'),
      key: 'since',
      cell: (a) => formatFullDate(a.since),
    },
    {
      header: t('admin.monitor.col.value'),
      key: 'value',
      cell: (a) => valueText(a),
    },
    {
      header: t('admin.monitor.col.firstStep'),
      key: 'step',
      cell: (a) => <span className="text-fg-secondary text-xs">{stepOf(lang, a)}</span>,
    },
  ]
  return (
    <AdminSection title={t('admin.monitor.alerts')}>
      {active.length === 0 && (
        <div className="p-4">
          <AdminEmptyState
            description={t('admin.monitor.noAlertsHelp')}
            title={t('admin.monitor.noAlerts')}
          />
        </div>
      )}
      {active.length > 0 && (
        <AdminTable
          columns={columns}
          label={t('admin.monitor.alerts')}
          rowKey={(a) => `${a.rule}/${a.object}`}
          rows={active}
        />
      )}
    </AdminSection>
  )
}

function phaseText(phase: string, t: ReturnType<typeof useT>): string {
  const key = phaseKey(phase)
  if (key === null) return phase
  return t(key)
}

/** The value against its threshold; why it is unknown when it could not be read. */
function valueText(a: WireOpsAlert): string {
  if (a.value !== null && a.threshold !== null) return `${a.value} / ${a.threshold}`
  if (a.value !== null) return a.value
  return a.why ?? '—'
}
