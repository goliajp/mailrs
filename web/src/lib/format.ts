import { currentLocale, tNow } from '@/i18n'

export function dateGroupLabel(epoch: number): string {
  const d = new Date(epoch * 1000)
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const msgDate = new Date(d.getFullYear(), d.getMonth(), d.getDate())
  const diffDays = Math.floor((today.getTime() - msgDate.getTime()) / 86400000)
  if (diffDays === 0) return tNow('date.today')
  if (diffDays === 1) return tNow('date.yesterday')
  if (diffDays < 7) return dateFormat(currentLocale(), { weekday: 'long' }).format(d)
  return dateFormat(currentLocale(), {
    day: 'numeric',
    month: 'short',
    year: now.getFullYear() !== d.getFullYear() ? 'numeric' : undefined,
  }).format(d)
}

export function formatDate(ts: number): string {
  const d = new Date(ts * 1000)
  const now = new Date()

  // today: show time
  if (d.toDateString() === now.toDateString()) {
    return dateFormat(currentLocale(), { hour: '2-digit', minute: '2-digit' }).format(d)
  }

  // yesterday
  if (isYesterday(d, now)) {
    return tNow('date.yesterday')
  }

  // same week (but not today/yesterday): show weekday
  if (isSameWeek(d, now)) {
    return dateFormat(currentLocale(), { weekday: 'short' }).format(d)
  }

  // same year: month + day
  if (d.getFullYear() === now.getFullYear()) {
    return dateFormat(currentLocale(), { day: 'numeric', month: 'short' }).format(d)
  }

  // older: abbreviated year
  return dateFormat(currentLocale(), {
    day: 'numeric',
    month: 'short',
    year: '2-digit',
  }).format(d)
}

export function formatFullDate(ts: number): string {
  return dateFormat(currentLocale(), {
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    month: 'short',
    weekday: 'short',
    year: 'numeric',
  }).format(new Date(ts * 1000))
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
}

/// Fixed-format HH:mm for timeline message headers. The surrounding date
/// divider already provides the day grouping, so the per-message timestamp
/// only carries the time-of-day signal — using `formatDate`'s fuzzy
/// (today→time / yesterday / weekday / month-day) output here would
/// duplicate the divider's "4月16日" label for every same-day message.
export function formatTimeOfDay(ts: number): string {
  return dateFormat(currentLocale(), {
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(ts * 1000))
}

export function formatUptime(secs: null | number | undefined): string {
  if (secs == null || !Number.isFinite(secs) || secs < 0) return '-'
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h}h ${m}m`
  return `${m}m ${s}s`
}

function isSameWeek(d: Date, now: Date): boolean {
  const nowStart = startOfDay(now)
  const dayOfWeek = nowStart.getDay()
  // week starts on Monday: go back (dayOfWeek - 1) days, or 6 if Sunday
  const mondayOffset = dayOfWeek === 0 ? 6 : dayOfWeek - 1
  const weekStart = new Date(nowStart)
  weekStart.setDate(weekStart.getDate() - mondayOffset)
  return startOfDay(d).getTime() >= weekStart.getTime() && d < now
}

function isYesterday(d: Date, now: Date): boolean {
  const yesterday = startOfDay(now)
  yesterday.setDate(yesterday.getDate() - 1)
  return startOfDay(d).getTime() === yesterday.getTime()
}

function startOfDay(date: Date): Date {
  const d = new Date(date)
  d.setHours(0, 0, 0, 0)
  return d
}

const formatters = new Map<string, Intl.DateTimeFormat>()

/**
 * One `Intl.DateTimeFormat` per locale and options, kept.
 *
 * `toLocaleString` with options builds a new formatter on every call, and
 * building one costs far more than using it. The conversation list formats
 * a date on every row it draws; when a scroll brings thirty rows in at
 * once, that was the largest single cost in the frame. Given the same
 * options, a formatter formats as `toLocaleString` does, and as
 * `toLocaleDateString` (`toLocaleTimeString`) does when the options name a
 * date (time) field and no time (date) field, which is how they were used.
 */
export function dateFormat(
  locale: string,
  options: Intl.DateTimeFormatOptions
): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`
  let f = formatters.get(key)
  if (!f) {
    f = new Intl.DateTimeFormat(locale, options)
    formatters.set(key, f)
  }
  return f
}
