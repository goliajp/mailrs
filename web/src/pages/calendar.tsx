import type { CalendarView } from '@/lib/calendar-grid'
import type { WireCalendarEvent } from '@/wire/schemas/calendar'

import { useQuery } from '@tanstack/react-query'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useSearchParams } from 'react-router'

import { EventDetail, MonthView, WeekView } from '@/components/calendar/calendar-views'
import { either, useLocale, useT } from '@/i18n'
import { daysOf, shift, startOfDay, windowOf } from '@/lib/calendar-grid'
import { calendarKeys } from '@/lib/query-keys'
import { wireCalendarEvents } from '@/wire/endpoints/calendar'

const NO_EVENTS: WireCalendarEvent[] = []

/**
 * The calendar, read only: the reader's events, invitations filed from
 * mail with their answer, and subscribed feeds. Which view, which date and
 * which event is open live in the URL, so a reload or a pasted link lands
 * on the same page.
 */
export function Calendar() {
  const t = useT()
  const locale = useLocale()
  const [params, setParams] = useSearchParams()
  const today = startOfDay(new Date())
  const view = viewOf(params.get('view'))
  const anchor = anchorOf(params.get('date'), today)
  const days = daysOf(view, anchor)
  const { end, start } = windowOf(view, anchor)

  const query = useQuery({
    queryKey: calendarKeys.events(start.toISOString(), end.toISOString()),
    queryFn: ({ signal }) => wireCalendarEvents(start, end, signal),
  })
  const events = query.data ?? NO_EVENTS
  const openUid = params.get('event')
  const open = events.find((ev) => ev.uid === openUid) ?? null

  const go = (next: Partial<Record<'date' | 'event' | 'view', null | string>>) => {
    const merged = new URLSearchParams(params)
    for (const [k, v] of Object.entries(next)) {
      if (v === null || v === undefined) merged.delete(k)
      else merged.set(k, v)
    }
    setParams(merged)
  }

  let heading = anchor.toLocaleDateString(locale, { month: 'long', year: 'numeric' })
  if (view === 'week') {
    const range = { day: 'numeric', month: 'short' } as const
    heading = `${days[0].toLocaleDateString(locale, range)} – ${days[6].toLocaleDateString(locale, range)}`
  }

  return (
    <div className="flex flex-col gap-3 p-3">
      <header className="flex flex-wrap items-center gap-2">
        <h1 className="text-fg mr-2 text-lg font-semibold">{t('calendar.title')}</h1>
        <button
          aria-label={t('calendar.previous')}
          className="text-fg-secondary hover:bg-bg-secondary rounded-md p-1.5"
          onClick={() => go({ date: dayParam(shift(view, anchor, -1)), event: null })}
          type="button"
        >
          <ChevronLeft className="h-4 w-4" />
        </button>
        <button
          className="text-fg-secondary hover:bg-bg-secondary rounded-md px-2 py-1 text-sm"
          onClick={() => go({ date: null, event: null })}
          type="button"
        >
          {t('calendar.today')}
        </button>
        <button
          aria-label={t('calendar.next')}
          className="text-fg-secondary hover:bg-bg-secondary rounded-md p-1.5"
          onClick={() => go({ date: dayParam(shift(view, anchor, 1)), event: null })}
          type="button"
        >
          <ChevronRight className="h-4 w-4" />
        </button>
        <span className="text-fg text-sm font-medium" data-testid="calendar-heading">
          {heading}
        </span>
        <div className="ml-auto flex gap-1" role="tablist">
          {(['week', 'month'] as const).map((v) => (
            <button
              aria-selected={view === v}
              className="aria-selected:bg-accent/10 aria-selected:text-accent text-fg-secondary rounded-md px-2 py-1 text-sm"
              key={v}
              onClick={() => go({ event: null, view: v })}
              role="tab"
              type="button"
            >
              {t(either(v === 'week', 'calendar.week', 'calendar.month'))}
            </button>
          ))}
        </div>
      </header>

      {query.isError && (
        <p className="text-danger text-sm" role="alert">
          {t('calendar.loadFailed')}
        </p>
      )}
      {query.isSuccess && events.length === 0 && (
        <p className="text-fg-muted text-sm">{t('calendar.noEvents')}</p>
      )}

      {open && <EventDetail ev={open} onClose={() => go({ event: null })} />}

      {view === 'week' && (
        <WeekView
          days={days}
          events={events}
          onOpen={(ev) => go({ event: ev.uid })}
          today={today}
        />
      )}
      {view === 'month' && (
        <MonthView
          anchor={anchor}
          days={days}
          events={events}
          onOpen={(ev) => go({ event: ev.uid })}
          today={today}
        />
      )}
    </div>
  )
}

/** The date in the URL, or today when there is none or it does not read. */
function anchorOf(param: null | string, today: Date): Date {
  const m = param?.match(/^(\d{4})-(\d{2})-(\d{2})$/)
  if (!m) return today
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
}

/** `YYYY-MM-DD` of a local date, for the URL. */
function dayParam(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

function viewOf(param: null | string): CalendarView {
  if (param === 'month') return 'month'
  return 'week'
}
