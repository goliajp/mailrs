import type { WireOpsDaemonState, WireOpsSilence } from '@/wire/schemas/ops'

import { Alert, Button } from '@goliapkg/gds'
import { useMutation } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { useState } from 'react'

import { AdminSection, ConfirmAction } from '@/components/admin-page'
import { langAtom, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { adminKeys } from '@/lib/query-keys'
import { wireOpsEndSilence, wireOpsSilence } from '@/wire/endpoints/ops'

import { DURATIONS, titleOf } from './texts'

const FIELD = 'border-border bg-bg-secondary rounded-md border px-3 py-1.5 text-sm'

/** The silences in force, ending one, and setting one: always with an end and a reason. */
export function SilencesPanel({ state }: { state: WireOpsDaemonState }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  const [ending, setEnding] = useState<null | WireOpsSilence>(null)
  const end = useMutation({
    mutationFn: (s: WireOpsSilence) => wireOpsEndSilence(s.id, 'ended from the admin page'),
    onSettled: () => {
      setEnding(null)
      refresh()
    },
  })
  const title = (name: string) => {
    const rule = state.rules.find((r) => r.name === name)
    if (rule === undefined) return name
    return titleOf(lang, rule)
  }
  return (
    <AdminSection title={t('admin.monitor.silences')}>
      <div className="space-y-3 p-4">
        {end.error && (
          <Alert role="alert" variant="danger">
            {t('admin.monitor.actionFailed', { message: end.error.message })}
          </Alert>
        )}
        {state.silences.length === 0 && (
          <p className="text-fg-muted text-sm">{t('admin.monitor.noSilences')}</p>
        )}
        <ul className="space-y-2">
          {state.silences.map((s) => (
            <li className="flex flex-wrap items-center justify-between gap-2 text-sm" key={s.id}>
              <span className="min-w-0 break-words">
                {t('admin.monitor.silenceRow', {
                  by: s.by,
                  object: s.object ?? t('admin.monitor.allObjects'),
                  reason: s.reason,
                  rule: title(s.rule),
                  until: formatFullDate(s.until),
                })}
              </span>
              <Button onClick={() => setEnding(s)} size="sm" variant="secondary">
                {t('admin.monitor.endSilence')}
              </Button>
            </li>
          ))}
        </ul>
        <SilenceForm state={state} />
      </div>
      {ending && (
        <ConfirmAction
          confirmLabel={t('admin.monitor.endSilence')}
          consequence={t('admin.monitor.endSilenceConsequence', { rule: title(ending.rule) })}
          onCancel={() => setEnding(null)}
          onConfirm={() => end.mutate(ending)}
          pending={end.isPending}
          title={t('admin.monitor.endSilence')}
        />
      )}
    </AdminSection>
  )
}

function refresh() {
  queryClient.invalidateQueries({ queryKey: adminKeys.opsState() })
  queryClient.invalidateQueries({ queryKey: adminKeys.opsHistory() })
}

function saveLabel(pending: boolean) {
  if (pending) return 'admin.monitor.saving' as const
  return 'admin.monitor.setSilence' as const
}

function SilenceForm({ state }: { state: WireOpsDaemonState }) {
  const t = useT()
  const lang = useAtomValue(langAtom)
  const [rule, setRule] = useState('')
  const [object, setObject] = useState('')
  const [forSecs, setForSecs] = useState(DURATIONS[0].secs)
  const [reason, setReason] = useState('')
  const set = useMutation({
    mutationFn: () =>
      wireOpsSilence({ forSecs, object: object.trim() || null, reason: reason.trim(), rule }),
    onSuccess: () => {
      setObject('')
      setReason('')
      refresh()
    },
  })
  const ready = rule !== '' && reason.trim() !== '' && !set.isPending
  return (
    <div className="border-border space-y-2 border-t pt-3">
      <div className="flex flex-wrap gap-2">
        <select
          aria-label={t('admin.monitor.silenceRule')}
          className={FIELD}
          onChange={(e) => setRule(e.target.value)}
          value={rule}
        >
          <option value="">{t('admin.monitor.silenceRule')}</option>
          {state.rules.map((r) => (
            <option key={r.name} value={r.name}>
              {titleOf(lang, r)}
            </option>
          ))}
        </select>
        <input
          aria-label={t('admin.monitor.silenceObject')}
          className={`${FIELD} flex-1`}
          onChange={(e) => setObject(e.target.value)}
          placeholder={t('admin.monitor.silenceObject')}
          value={object}
        />
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
      </div>
      <input
        aria-label={t('admin.monitor.reason')}
        className={`${FIELD} w-full`}
        onChange={(e) => setReason(e.target.value)}
        placeholder={t('admin.monitor.reason')}
        value={reason}
      />
      {set.error && (
        <Alert role="alert" variant="danger">
          {t('admin.monitor.actionFailed', { message: set.error.message })}
        </Alert>
      )}
      <Button disabled={!ready} onClick={() => set.mutate()} size="sm" variant="primary">
        {t(saveLabel(set.isPending))}
      </Button>
    </div>
  )
}
