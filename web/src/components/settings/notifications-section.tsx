import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { wireNotifyPrefs, wireNotifyStats, wirePutNotifyPrefs } from '@/wire/endpoints/notify'
import { wireWebPushState } from '@/wire/endpoints/web-push'
import { type WireNotifyPrefs } from '@/wire/schemas/notify'

import { btnPrimary, cardClass, Field, inputClass, SectionHeader, Toggle } from './_shared'
import { ImportantCard, OverridesCard, WebPushCard } from './notify-cards'
import { clock, minutesOf, WEB_PUSH_KEY, weekTotals } from './notify-shared'

const PREFS_KEY = ['settings', 'notify'] as const

const MODES: { key: Key; value: WireNotifyPrefs['mode'] }[] = [
  { key: 'settings.notify.mode.default', value: 'default' },
  { key: 'settings.notify.mode.all', value: 'all' },
  { key: 'settings.notify.mode.important', value: 'important' },
  { key: 'settings.notify.mode.off', value: 'off' },
]

/** When mail rings the reader's devices, and how often it did. */
export function NotificationsSection() {
  return (
    <div className="space-y-6">
      <PrefsCard />
      <WebPushCard />
      <ImportantCard />
      <OverridesCard />
      <StatsCard />
    </div>
  )
}

function PrefsCard() {
  const t = useT()
  const { data } = useQuery({ queryFn: wireNotifyPrefs, queryKey: PREFS_KEY })
  const [prefs, setPrefs] = useState<null | WireNotifyPrefs>(null)
  useEffect(() => {
    if (data) setPrefs(data)
  }, [data])
  if (!prefs) return null
  const quiet = prefs.quiet_start !== undefined && prefs.quiet_end !== undefined

  const setQuiet = (on: boolean) => {
    if (on) {
      setPrefs({ ...prefs, quiet_end: 7 * 60, quiet_start: 22 * 60 })
      return
    }
    setPrefs({ ...prefs, quiet_end: undefined, quiet_start: undefined })
  }

  const save = async () => {
    try {
      // the quiet hours are the reader's local time, so the offset is this browser's
      await wirePutNotifyPrefs({ ...prefs, utc_offset_minutes: -new Date().getTimezoneOffset() })
      toast.success(t('settings.notify.saved'))
      void queryClient.invalidateQueries({ queryKey: PREFS_KEY })
    } catch (e) {
      toast.error(errorText(e, t('settings.notify.failed')))
    }
  }

  return (
    <section className={cardClass}>
      <SectionHeader title={t('settings.notify.mode')} />
      <p className="text-fg-muted mb-3 text-sm">{t('settings.notify.blurb')}</p>
      <fieldset className="mb-4 space-y-2">
        {MODES.map((m) => (
          <label className="flex items-center gap-2 text-sm" key={m.value}>
            <input
              checked={prefs.mode === m.value}
              name="notify-mode"
              onChange={() => setPrefs({ ...prefs, mode: m.value })}
              type="radio"
            />
            {t(m.key)}
          </label>
        ))}
      </fieldset>
      <div className="space-y-3">
        <Field label={t('settings.notify.quiet')}>
          <Toggle checked={quiet} onChange={setQuiet} />
        </Field>
        {quiet && (
          <div className="flex items-center gap-2 text-sm">
            <span>{t('settings.notify.quietFrom')}</span>
            <input
              aria-label={t('settings.notify.quietFrom')}
              className={inputClass}
              onChange={(e) => setPrefs({ ...prefs, quiet_start: minutesOf(e.target.value) })}
              type="time"
              value={clock(prefs.quiet_start ?? 0)}
            />
            <span>{t('settings.notify.quietTo')}</span>
            <input
              aria-label={t('settings.notify.quietTo')}
              className={inputClass}
              onChange={(e) => setPrefs({ ...prefs, quiet_end: minutesOf(e.target.value) })}
              type="time"
              value={clock(prefs.quiet_end ?? 0)}
            />
          </div>
        )}
        <Field label={t('settings.notify.hideContent')}>
          <Toggle
            checked={prefs.hide_content}
            onChange={(v) => setPrefs({ ...prefs, hide_content: v })}
          />
        </Field>
        <p className="text-fg-muted text-xs">{t('settings.notify.hideContentBlurb')}</p>
        <button className={btnPrimary} onClick={() => void save()}>
          {t('settings.notify.save')}
        </button>
      </div>
    </section>
  )
}

const CHANNELS: { key: Key; name: string }[] = [
  { key: 'settings.notify.channel.apns', name: 'apns' },
  { key: 'settings.notify.channel.web', name: 'web' },
]

function StatsCard() {
  const t = useT()
  const { data: days = [] } = useQuery({
    queryKey: ['settings', 'notify-stats'],
    queryFn: () => wireNotifyStats(7),
  })
  const { data: web } = useQuery({ queryFn: wireWebPushState, queryKey: WEB_PUSH_KEY })
  return (
    <section className={cardClass}>
      <SectionHeader title={t('settings.notify.stats')} />
      <ul className="space-y-1 text-sm">
        {CHANNELS.map((c) => {
          const n = weekTotals(days, c.name)
          // nothing could be pushed: say so rather than show zeros
          if (c.name === 'web' && web?.enabled === false) {
            return (
              <li key={c.name}>
                <span className="font-medium">{t(c.key)}</span> {t('settings.notify.webOff')}
              </li>
            )
          }
          return (
            <li key={c.name}>
              <span className="font-medium">{t(c.key)}</span>{' '}
              {t('settings.notify.statsLine', {
                clicked: n.clicked ?? 0,
                failed: n.failed ?? 0,
                policy: n.suppressed_policy ?? 0,
                quiet: n.suppressed_quiet ?? 0,
                sent: n.sent ?? 0,
              })}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
