import { type SenderRuleBody } from '@/wire/endpoints/sender-rules'
import { type WireNotifyStats } from '@/wire/schemas/notify'
import { type SenderBucket } from '@/wire/schemas/sender-rules'

export const WEB_PUSH_KEY = ['settings', 'web-push'] as const

/** The sender rule form, as typed. */
export type Draft = {
  address: string
  bucket: '' | SenderBucket
  days: string
  disposal: 'archive' | 'trash'
  keep: string
}

export const EMPTY_DRAFT: Draft = {
  address: '',
  bucket: '',
  days: '',
  disposal: 'archive',
  keep: '',
}

/** Minutes after midnight as `HH:MM`. */
export function clock(minutes: number): string {
  const h = String(Math.floor(minutes / 60)).padStart(2, '0')
  const m = String(minutes % 60).padStart(2, '0')
  return `${h}:${m}`
}

/** The body a draft saves; null when it says nothing. */
export function draftBody(d: Draft): null | SenderRuleBody {
  const body: SenderRuleBody = {
    bucket: d.bucket || undefined,
    disposal: d.disposal,
    keep_latest: count(d.keep),
    older_than_days: count(d.days),
  }
  if (!body.bucket && body.keep_latest === undefined && body.older_than_days === undefined)
    return null
  return body
}

/** `HH:MM` as minutes after midnight. */
export function minutesOf(text: string): number {
  const [h, m] = text.split(':').map(Number)
  return h * 60 + m
}

/** The week's counters for one channel, summed. */
export function weekTotals(days: WireNotifyStats, channel: string): Record<string, number> {
  const out: Record<string, number> = {}
  for (const day of days) {
    for (const [outcome, n] of Object.entries(day.counts[channel] ?? {})) {
      out[outcome] = (out[outcome] ?? 0) + n
    }
  }
  return out
}

/** A positive whole number from a form field, or nothing. */
function count(text: string): number | undefined {
  const n = Number(text.trim())
  if (text.trim() === '' || !Number.isInteger(n) || n <= 0) return undefined
  return n
}
