import type { WireOpsState } from '@/wire/schemas/ops'

import { Alert } from '@goliapkg/gds'

import { useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'

type Reached = Extract<WireOpsState, { reach: 'ok' }>
type Unreached = Exclude<WireOpsState, { reach: 'ok' }>

/** Why there is nothing to show: said plainly, never an empty list. */
export function DaemonUnreached({ state }: { state: Unreached }) {
  const t = useT()
  if (state.reach === 'not_configured') {
    return (
      <Alert role="alert" title={t('admin.monitor.notConfigured.title')} variant="danger">
        <p className="text-sm">{t('admin.monitor.notConfigured.body')}</p>
      </Alert>
    )
  }
  if (state.reach === 'refused') {
    return (
      <Alert role="alert" title={t('admin.monitor.refused.title')} variant="danger">
        <p className="text-sm">{t('admin.monitor.refused.body')}</p>
      </Alert>
    )
  }
  return (
    <Alert role="alert" title={t('admin.monitor.unreachable.title')} variant="danger">
      <p className="text-sm">{t('admin.monitor.unreachable.body', { detail: state.detail })}</p>
    </Alert>
  )
}

/**
 * The daemon has not completed a check within MAILRS_OPS_SILENT_SECS, and
 * how far its alerts can reach: no outside watch, no external channel.
 */
export function DaemonWarnings({ state }: { state: Reached }) {
  const t = useT()
  const s = state.state
  return (
    <>
      {state.stale && (
        <Alert role="alert" title={staleTitle(s.last_cycle, t)} variant="danger">
          <p className="text-sm">
            {t('admin.monitor.stale.body', { secs: state.silent_after_secs })}
          </p>
        </Alert>
      )}
      {!s.external_channel && (
        <Alert role="alert" title={t('admin.monitor.noExternal.title')} variant="warning">
          <p className="text-sm">{t('admin.monitor.noExternal.line1')}</p>
          <p className="text-sm">{t('admin.monitor.noExternal.line2')}</p>
        </Alert>
      )}
      {s.unmonitored && (
        <Alert role="alert" title={t('admin.monitor.unmonitored.title')} variant="warning">
          <p className="text-sm">{t('admin.monitor.unmonitored.body')}</p>
        </Alert>
      )}
    </>
  )
}

function staleTitle(last: null | { started_at: number }, t: ReturnType<typeof useT>): string {
  if (last === null) return t('admin.monitor.stale.never')
  return t('admin.monitor.stale.title', { at: formatFullDate(last.started_at) })
}
