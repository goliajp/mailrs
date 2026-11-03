import type { WireCalendarEvent } from '@/wire/schemas/calendar'

/** Which grid the calendar draws. */
export type CalendarView = 'month' | 'week'

/** When an event is, as instants in the reader's own zone. */
export type Span = { allDay: boolean; end: number; start: number }

/** The days the view shows: one week, or the six weeks covering the month. */
export function daysOf(view: CalendarView, anchor: Date): Date[] {
  let first = startOfWeek(anchor)
  let count = 7
  if (view === 'month') {
    first = startOfWeek(new Date(anchor.getFullYear(), anchor.getMonth(), 1))
    count = 42
  }
  return Array.from({ length: count }, (_, i) => addDays(first, i))
}

/** The events touching `day`, earliest first. */
export function eventsOn(events: WireCalendarEvent[], day: Date): WireCalendarEvent[] {
  const from = day.getTime()
  // a day is not always 24 hours where the clocks change
  const to = addDays(day, 1).getTime()
  return events
    .filter((ev) => {
      const span = spanOf(ev)
      return span !== null && span.start < to && span.end > from
    })
    .sort((a, b) => (spanOf(a)?.start ?? 0) - (spanOf(b)?.start ?? 0))
}

/** Whether two dates fall on the same local day. */
export function sameDay(a: Date, b: Date): boolean {
  return startOfDay(a).getTime() === startOfDay(b).getTime()
}

/** One page back (`-1`) or forward (`1`): a week, or a month. */
export function shift(view: CalendarView, anchor: Date, step: -1 | 1): Date {
  if (view === 'week') return addDays(anchor, 7 * step)
  return new Date(anchor.getFullYear(), anchor.getMonth() + step, 1)
}

/**
 * Start and end instants; an event with no end lasts an hour, as the server
 * reads it. An all-day event comes as dates (`YYYY-MM-DD`, end exclusive)
 * and covers those days from local midnight to local midnight, wherever
 * the reader is.
 */
export function spanOf(ev: WireCalendarEvent): null | Span {
  if (!ev.dtstart) return null
  const firstDay = localDate(ev.dtstart)
  if (firstDay) {
    const afterLast = localDate(ev.dtend ?? '') ?? addDays(firstDay, 1)
    return { allDay: true, end: afterLast.getTime(), start: firstDay.getTime() }
  }
  const start = Date.parse(ev.dtstart)
  if (Number.isNaN(start)) return null
  let end = start + 3_600_000
  if (ev.dtend) {
    const parsed = Date.parse(ev.dtend)
    if (!Number.isNaN(parsed)) end = parsed
  }
  return { allDay: false, end, start }
}

/** Local midnight of `d`. */
export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate())
}

/** The Monday on or before `d`, at local midnight. Weeks start on Monday. */
export function startOfWeek(d: Date): Date {
  const day = startOfDay(d)
  const sinceMonday = (day.getDay() + 6) % 7
  return addDays(day, -sinceMonday)
}

/** The window to ask the server for: the first shown day to the day after the last. */
export function windowOf(view: CalendarView, anchor: Date): { end: Date; start: Date } {
  const days = daysOf(view, anchor)
  return { end: addDays(days[days.length - 1], 1), start: days[0] }
}

function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n)
}

/** Local midnight of a `YYYY-MM-DD` date, or null for anything else. */
function localDate(value: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value)
  if (!m) return null
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
}
