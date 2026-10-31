import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useT } from '@/i18n'
import { queryClient } from '@/lib/query-client'
import { wireGetVacation, wireSetVacation } from '@/wire/endpoints/mail-settings'

import { btnPrimary, cardClass, inputClass, Toggle } from './_shared'
import { errorText } from './rules-shared'

const vacationKey = ['settings', 'vacation'] as const

/** The vacation reply: once per sender within the days given. */
export function VacationCard() {
  const { data } = useQuery({
    queryKey: vacationKey,
    queryFn: ({ signal }) => wireGetVacation(signal),
  })
  if (!data) return null
  return <VacationForm initial={data} key={data.version} />
}

function dateInput(at: null | number): string {
  if (at === null) return ''
  return new Date(at * 1000).toISOString().slice(0, 10)
}

function epoch(day: string): null | number {
  if (!day) return null
  return Math.floor(Date.parse(`${day}T00:00:00Z`) / 1000)
}

function VacationForm({
  initial,
}: {
  initial: {
    body: string
    days: number
    enabled: boolean
    end: null | number
    start: null | number
    subject: string
  }
}) {
  const t = useT()
  const [enabled, setEnabled] = useState(initial.enabled)
  const [subject, setSubject] = useState(initial.subject)
  const [body, setBody] = useState(initial.body)
  const [days, setDays] = useState(initial.days)
  const [start, setStart] = useState(dateInput(initial.start))
  const [end, setEnd] = useState(dateInput(initial.end))
  const save = async () => {
    try {
      await wireSetVacation({
        body,
        days,
        enabled,
        end: epoch(end),
        start: epoch(start),
        subject,
      })
      toast.success(t('settings.vacation.saved'))
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void queryClient.invalidateQueries({ queryKey: vacationKey })
    }
  }
  return (
    <form
      className={`${cardClass} space-y-3 text-sm`}
      onSubmit={(e) => {
        e.preventDefault()
        void save()
      }}
    >
      <div className="flex items-center justify-between">
        <h3 className="font-medium">{t('settings.vacation.title')}</h3>
        <Toggle checked={enabled} onChange={setEnabled} />
      </div>
      <label className="block space-y-1">
        <span className="text-fg-secondary">{t('settings.vacation.subject')}</span>
        <input
          className={inputClass}
          maxLength={200}
          onChange={(e) => setSubject(e.target.value)}
          value={subject}
        />
      </label>
      <label className="block space-y-1">
        <span className="text-fg-secondary">{t('settings.vacation.message')}</span>
        <textarea
          className={inputClass}
          onChange={(e) => setBody(e.target.value)}
          rows={4}
          value={body}
        />
      </label>
      <div className="flex flex-wrap gap-3">
        <label className="space-y-1">
          <span className="text-fg-secondary block">{t('settings.vacation.from')}</span>
          <input
            className={inputClass}
            onChange={(e) => setStart(e.target.value)}
            type="date"
            value={start}
          />
        </label>
        <label className="space-y-1">
          <span className="text-fg-secondary block">{t('settings.vacation.until')}</span>
          <input
            className={inputClass}
            onChange={(e) => setEnd(e.target.value)}
            type="date"
            value={end}
          />
        </label>
        <label className="space-y-1">
          <span className="text-fg-secondary block">{t('settings.vacation.every')}</span>
          <select
            className={inputClass}
            onChange={(e) => setDays(Number(e.target.value))}
            value={days}
          >
            {[1, 3, 7, 14, 30].map((d) => (
              <option key={d} value={d}>
                {t('settings.vacation.days', { count: d })}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="text-fg-muted text-xs">{t('settings.vacation.note')}</p>
      <div className="flex justify-end">
        <button className={btnPrimary} type="submit">
          {t('settings.save')}
        </button>
      </div>
    </form>
  )
}
