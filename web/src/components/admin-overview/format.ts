import type { Key } from '@/i18n'

export type Translate = (key: Key, vars?: Record<string, number | string>) => string

/** A count, or a dash when it is not known. */
export function count(n: null | number): string {
  if (n === null) return '—'
  return n.toLocaleString('en-US')
}

/** `3h 4m`, `2d 1h`, `5m`. */
export function uptime(secs: number, t: Translate): string {
  const days = Math.floor(secs / 86400)
  const hours = Math.floor((secs % 86400) / 3600)
  const mins = Math.floor((secs % 3600) / 60)
  if (days > 0) return t('admin.uptime.days', { d: days, h: hours })
  if (hours > 0) return t('admin.uptime.hours', { h: hours, m: mins })
  return t('admin.uptime.minutes', { m: mins })
}
