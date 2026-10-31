/**
 * Timestamp formatting for the API Keys table.
 *
 * Backend: `crates/webapi/src/handlers/complete.rs:1339` writes
 * `created_at: now_secs()` — unix epoch SECONDS, which the wire schema
 * stringifies. Anything else (missing field, empty string) is treated as
 * unknown and rendered as an em dash rather than as 1970.
 */

import { getDefaultStore } from 'jotai'

import { type Key, langAtom, translate } from '@/i18n'

const MINUTE = 60
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const MONTH = 30 * DAY
const YEAR = 365 * DAY

/** `2026-07-18 14:20` in the viewer's local time — sortable at a glance. */
export function formatAbsolute(date: Date): string {
  const y = date.getFullYear()
  const mo = pad(date.getMonth() + 1)
  const d = pad(date.getDate())
  const h = pad(date.getHours())
  const mi = pad(date.getMinutes())
  return `${y}-${mo}-${d} ${h}:${mi}`
}

export function formatRelative(date: Date, now: Date): string {
  const secs = Math.round((now.getTime() - date.getTime()) / 1000)
  if (secs < MINUTE) return ago('settings.ago.now', 0)
  if (secs < HOUR) return ago('settings.ago.minutes', Math.floor(secs / MINUTE))
  if (secs < DAY) return ago('settings.ago.hours', Math.floor(secs / HOUR))
  if (secs < MONTH) return ago('settings.ago.days', Math.floor(secs / DAY))
  if (secs < YEAR) return ago('settings.ago.months', Math.floor(secs / MONTH))
  return ago('settings.ago.years', Math.floor(secs / YEAR))
}

export function parseEpochSeconds(raw: string): Date | null {
  const n = Number(raw)
  if (!Number.isFinite(n) || n <= 0) return null
  return new Date(n * 1000)
}

function ago(key: Key, count: number): string {
  return translate(getDefaultStore().get(langAtom), key, { count })
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}
