// A recurring invitation, as a person reads it: how it repeats, in words,
// and when it is next. The rule is expanded on the server, which adds
// `recurrence` and `next_occurrence` to the payload when the message is
// read (crates/core-sidestate/src/families/calendar_events/series.rs —
// fn annotate); nothing here works out an occurrence or an offset.
import type { Key, Translate } from '@/i18n'
import type { CalDateTime } from '@/lib/invite-time'
import type { InvitePayload } from '@/lib/invite-types'

/// The first occurrence that has not ended.
export type NextOccurrence = {
  /// RFC 3339, or `YYYY-MM-DD` for an all-day series (end exclusive).
  end: string
  /// The organiser's wall-clock then, when their zone is known.
  organiser_local: null | string
  organiser_offset_secs: null | number
  start: string
}

/// The rule, read for showing in words (mailrs_ical::recur::Rule).
export type RecurrenceRule = {
  by_day: string[]
  by_month: number[]
  by_month_day: number[]
  count: null | number
  freq: string
  interval: number
  plain: boolean
  /// The last day, `YYYY-MM-DD`, in the organiser's zone.
  until: null | string
}

/// The span the card shows: the next occurrence of a recurring invitation,
/// otherwise the event itself. Instants arrive resolved from the server;
/// the wall-clock is used only when there is none (all-day, floating, or
/// mail stored before the instants were).
export function shownSpan(p: InvitePayload): {
  end: CalDateTime | null | undefined
  start: CalDateTime | null | undefined
} {
  const next = p.next_occurrence
  if (next) return { end: columnAt(next.end), start: columnAt(next.start) }
  return {
    end: instantOr(p.dtend_utc, p.dtend),
    start: instantOr(p.dtstart_utc, p.dtstart),
  }
}

function columnAt(column: string): CalDateTime {
  if (column.length === 10) return { Date: column }
  return { Utc: column }
}

function instantOr(
  utc: null | string | undefined,
  wall: CalDateTime | null | undefined
): CalDateTime | null | undefined {
  if (utc) return { Utc: utc }
  return wall
}

const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'] as const

type Day = (typeof DAYS)[number]

/// A `YYYY-MM-DD` as the reader writes a date. A date has no zone, so it
/// is formatted as itself, never moved through an offset.
export function formatDay(day: string, locale: string): string {
  const d = new Date(`${day}T00:00:00Z`)
  if (isNaN(d.getTime())) return day
  return d.toLocaleDateString(locale, { dateStyle: 'medium', timeZone: 'UTC' })
}

/// "每周四 · 至 2027年4月29日" — the rule in the reader's language. The
/// weekdays are the organiser's: "every Thursday" in Santa Clara is
/// Friday morning in Tokyo, which is why the card shows the next one as a
/// date in the reader's own zone beside it.
export function recurrenceText(
  rule: RecurrenceRule,
  t: Translate,
  locale: string,
  firstDay: null | string
): string {
  const parts = [ruleWords(rule, t, firstDay)]
  if (rule.until) parts.push(t('invite.repeat.until', { date: formatDay(rule.until, locale) }))
  if (rule.count !== null) parts.push(t('invite.repeat.count', { n: rule.count }))
  return parts.join(' · ')
}

/// The weekday code (`TH`) of a `YYYY-MM-DD…` wall-clock date.
export function weekdayOf(local: null | string): Day | null {
  if (!local) return null
  const d = new Date(`${local.slice(0, 10)}T00:00:00Z`)
  if (isNaN(d.getTime())) return null
  return DAYS[(d.getUTCDay() + 6) % 7]
}

function days(codes: string[], t: Translate): null | string {
  const named = codes.filter((c): c is Day => (DAYS as readonly string[]).includes(c))
  if (named.length === 0 || named.length !== codes.length) return null
  return named.map((c) => t(`invite.day.${c}` as Key)).join(t('invite.repeat.daySeparator'))
}

function ruleWords(rule: RecurrenceRule, t: Translate, firstDay: null | string): string {
  if (!rule.plain || rule.interval < 1) return t('invite.repeat.other')
  const n = rule.interval
  switch (rule.freq) {
    case 'DAILY':
      if (n === 1) return t('invite.repeat.daily')
      return t('invite.repeat.dailyN', { n })
    case 'MONTHLY':
      if (n === 1 && rule.by_month_day.length === 1 && rule.by_day.length === 0) {
        return t('invite.repeat.monthlyDay', { d: rule.by_month_day[0] })
      }
      if (n === 1) return t('invite.repeat.monthly')
      return t('invite.repeat.other')
    case 'WEEKLY':
      return weekly(rule, t, firstDay)
    case 'YEARLY':
      if (n === 1) return t('invite.repeat.yearly')
      return t('invite.repeat.other')
    default:
      return t('invite.repeat.other')
  }
}

function weekly(rule: RecurrenceRule, t: Translate, firstDay: null | string): string {
  const named = days(weeklyDays(rule, firstDay), t)
  if (!named) return t('invite.repeat.other')
  if (rule.interval === 1) return t('invite.repeat.weekly', { days: named })
  return t('invite.repeat.weeklyN', { days: named, n: rule.interval })
}

/// A weekly rule without `BYDAY` repeats on the first occurrence's weekday
/// (RFC 5545 §3.3.10).
function weeklyDays(rule: RecurrenceRule, firstDay: null | string): string[] {
  if (rule.by_day.length > 0) return rule.by_day
  const first = weekdayOf(firstDay)
  if (!first) return []
  return [first]
}
