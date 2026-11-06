import type { MaintenanceAction } from '@/wire/endpoints/ops'
import type { WireOpsDaemonState } from '@/wire/schemas/ops'

import { Alert, Button } from '@goliapkg/gds'
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'

import { AdminSection, ConfirmAction } from '@/components/admin-page'
import { useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { adminKeys } from '@/lib/query-keys'
import { wireOpsMaintenance } from '@/wire/endpoints/ops'

import { DURATIONS } from './texts'

const FIELD = 'border-border bg-bg-secondary rounded-md border px-3 py-1.5 text-sm'

/**
 * Maintenance: on until when and by whom, how many notifications it holds;
 * start or extend it (always with an end) or end it, each with a reason.
 */
export function MaintenancePanel({ state }: { state: WireOpsDaemonState }) {
  const t = useT()
  const [forSecs, setForSecs] = useState(DURATIONS[0].secs)
  const [reason, setReason] = useState('')
  const [confirmEnd, setConfirmEnd] = useState(false)
  const change = useMutation({
    // the endpoint leaves the length out when ending
    mutationFn: (action: MaintenanceAction) => wireOpsMaintenance(action, reason.trim(), forSecs),
    onSettled: () => {
      setConfirmEnd(false)
      queryClient.invalidateQueries({ queryKey: adminKeys.opsState() })
      queryClient.invalidateQueries({ queryKey: adminKeys.opsHistory() })
    },
    onSuccess: () => setReason(''),
  })
  const m = state.maintenance
  const ready = reason.trim() !== '' && !change.isPending
  return (
    <AdminSection title={t('admin.monitor.maintenance')}>
      <div className="space-y-3 p-4 text-sm">
        {m === null && <p>{t('admin.monitor.maintenanceOff')}</p>}
        {m !== null && (
          <div className="space-y-1">
            <p>
              {t('admin.monitor.maintenanceOn', {
                by: m.by,
                reason: m.reason,
                until: formatFullDate(m.until),
              })}
            </p>
            <p className="text-fg-secondary">
              {t('admin.monitor.heldCount', { n: state.notifications.held_for_maintenance })}
            </p>
          </div>
        )}
        <p className="text-fg-muted text-xs">{t('admin.monitor.maintenanceHelp')}</p>
        <div className="flex flex-wrap gap-2">
          <select
            aria-label={t('admin.monitor.duration')}
            className={FIELD}
            onChange={(e) => setForSecs(Number(e.target.value))}
            value={forSecs}
          >
            {DURATIONS.map((d) => (
              <option key={d.secs} value={d.secs}>
                {t(d.key)}
              </option>
            ))}
          </select>
          <input
            aria-label={t('admin.monitor.reason')}
            className={`${FIELD} flex-1`}
            onChange={(e) => setReason(e.target.value)}
            placeholder={t('admin.monitor.reason')}
            value={reason}
          />
        </div>
        {change.error && (
          <Alert role="alert" variant="danger">
            {t('admin.monitor.actionFailed', { message: change.error.message })}
          </Alert>
        )}
        <div className="flex flex-wrap gap-2">
          {m === null && (
            <Button
              disabled={!ready}
              onClick={() => change.mutate('start')}
              size="sm"
              variant="primary"
            >
              {t('admin.monitor.startMaintenance')}
            </Button>
          )}
          {m !== null && (
            <>
              <Button
                disabled={!ready}
                onClick={() => change.mutate('extend')}
                size="sm"
                variant="secondary"
              >
                {t('admin.monitor.extendMaintenance')}
              </Button>
              <Button
                disabled={!ready}
                onClick={() => setConfirmEnd(true)}
                size="sm"
                variant="secondary"
              >
                {t('admin.monitor.endMaintenance')}
              </Button>
            </>
          )}
        </div>
      </div>
      {confirmEnd && (
        <ConfirmAction
          confirmLabel={t('admin.monitor.endMaintenance')}
          consequence={t('admin.monitor.endMaintenanceConsequence')}
          onCancel={() => setConfirmEnd(false)}
          onConfirm={() => change.mutate('end')}
          pending={change.isPending}
          title={t('admin.monitor.endMaintenance')}
        />
      )}
    </AdminSection>
  )
}
