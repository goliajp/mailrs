import type { WireOpsDaemonState, WireOpsHistory } from '@/wire/schemas/ops'

import { useAtomValue } from 'jotai'

import { AdminSection } from '@/components/admin-page'
import { langAtom, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'

import { ACCEPTED_250, phaseKey, titleOf } from './texts'

type Rules = WireOpsDaemonState['rules']

/**
 * What went through mailrs's own path: queued by the core, then accepted
 * by the receiving server (250) once the sender's heartbeat lists it.
 */
export function OwnPath({ awaiting, history }: { awaiting: string[]; history: WireOpsHistory }) {
  const t = useT()
  const own = history.notifications.filter((n) => n.channel === 'own')
  return (
    <AdminSection title={t('admin.monitor.ownPath')}>
      <div className="space-y-2 p-4 text-sm">
        {awaiting.length > 0 && <p>{t('admin.monitor.awaiting250', { n: awaiting.length })}</p>}
        <ul className="space-y-1">
          {own.map((n) => (
            <li className="flex flex-wrap gap-x-3" key={`${n.group_id}/${n.at}/${n.result}`}>
              <span className="text-fg-muted tabular-nums">{formatFullDate(n.at)}</span>
              <span>{n.summary}</span>
              <span className="text-fg-secondary">{ownResult(n.result, t)}</span>
            </li>
          ))}
        </ul>
        <p className="text-fg-muted text-xs">{t('admin.monitor.own250Note')}</p>
      </div>
    </AdminSection>
  )
}

/** The last day's phase changes, newest first. */
export function Transitions({ history, rules }: { history: WireOpsHistory; rules: Rules }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  const title = (name: string) => {
    const rule = rules.find((r) => r.name === name)
    if (rule === undefined) return name
    return titleOf(lang, rule)
  }
  const phase = (p: string) => {
    const key = phaseKey(p)
    if (key === null) return p
    return t(key)
  }
  return (
    <AdminSection title={t('admin.monitor.transitions')}>
      {history.transitions.length === 0 && (
        <p className="text-fg-muted p-4 text-sm">{t('admin.monitor.noTransitions')}</p>
      )}
      <ul className="divide-border divide-y">
        {history.transitions.map((x) => (
          <li
            className="flex flex-wrap items-baseline gap-x-3 px-4 py-2 text-sm"
            key={`${x.rule}/${x.object}/${x.at}/${x.to}`}
          >
            <span className="text-fg-muted tabular-nums">{formatFullDate(x.at)}</span>
            <span className="font-medium">{title(x.rule)}</span>
            <span className="break-all">{x.object}</span>
            <span>{`${phase(x.from)} → ${phase(x.to)}`}</span>
            {x.value !== null && <span className="text-fg-secondary">{x.value}</span>}
          </li>
        ))}
      </ul>
    </AdminSection>
  )
}

function ownResult(result: string, t: ReturnType<typeof useT>): string {
  if (result === ACCEPTED_250) return t('admin.monitor.accepted250')
  if (result === 'sent') return t('admin.monitor.queuedOwn')
  return result
}
