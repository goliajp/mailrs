export function dateGroupLabel(epoch: number): string {
  const d = new Date(epoch * 1000)
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const msgDate = new Date(d.getFullYear(), d.getMonth(), d.getDate())
  const diffDays = Math.floor((today.getTime() - msgDate.getTime()) / 86400000)
  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays < 7) return d.toLocaleDateString(undefined, { weekday: 'long' })
  return d.toLocaleDateString(undefined, {
    day: 'numeric',
    month: 'short',
    year: now.getFullYear() !== d.getFullYear() ? 'numeric' : undefined,
  })
}

export function formatDate(ts: number): string {
  const d = new Date(ts * 1000)
  const now = new Date()

  // today: show time
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }

  // yesterday
  if (isYesterday(d, now)) {
    return 'Yesterday'
  }

  // same week (but not today/yesterday): show weekday
  if (isSameWeek(d, now)) {
    return d.toLocaleDateString([], { weekday: 'short' })
  }

  // same year: month + day
  if (d.getFullYear() === now.getFullYear()) {
    return d.toLocaleDateString([], { day: 'numeric', month: 'short' })
  }

  // older: abbreviated year
  return d.toLocaleDateString([], {
    day: 'numeric',
    month: 'short',
    year: '2-digit',
  })
}

export function formatFullDate(ts: number): string {
  return new Date(ts * 1000).toLocaleString([], {
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    month: 'short',
    weekday: 'short',
    year: 'numeric',
  })
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
  return new Date(ts * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  })
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
