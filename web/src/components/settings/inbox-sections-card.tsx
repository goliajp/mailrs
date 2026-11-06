import { toast } from '@goliapkg/gds'
import { useAtom } from 'jotai'

import { useSectionedInbox, useSetSectionedInbox } from '@/hooks/use-sectioned-inbox'
import { useT } from '@/i18n'
import { FOLLOWUP_DAY_CHOICES, followupDaysAtom } from '@/store/inbox-sections'

import { cardClass, Field, Toggle } from './_shared'
import { errorText } from './rules-shared'

/**
 * The sectioned inbox: on or off, saved on the server for the phone apps
 * too, and when a follow-up is due, kept in this browser.
 */
export function InboxSectionsCard() {
  const t = useT()
  const on = useSectionedInbox()
  const save = useSetSectionedInbox()
  const [days, setDays] = useAtom(followupDaysAtom)
  const turn = (value: boolean) => save.mutate(value, { onError: (e) => toast.error(errorText(e)) })
  return (
    <div className={cardClass}>
      <div className="space-y-3">
        <Field label={t('settings.sections.toggle')}>
          <Toggle checked={on} onChange={turn} />
        </Field>
        <Field label={t('settings.sections.days')}>
          <label className="flex items-center gap-2 text-sm">
            <select
              aria-label={t('settings.sections.days')}
              className="border-border bg-surface rounded-md border px-2 py-1"
              onChange={(e) => setDays(Number(e.target.value))}
              value={days}
            >
              {FOLLOWUP_DAY_CHOICES.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
            {t('settings.sections.daysUnit')}
          </label>
        </Field>
        <p className="text-fg-muted text-xs">{t('settings.sections.help')}</p>
      </div>
    </div>
  )
}
