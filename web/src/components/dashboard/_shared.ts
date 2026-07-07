// shared pure helpers + color maps used by multiple dashboard widgets.

import { decodeMimeHeader } from '@/lib/avatar'

export function extractEmail(sender: string): string {
  const decoded = decodeMimeHeader(sender)
  const m = decoded.match(/<([^>]+)>/)
  return m ? m[1] : decoded
}

export function extractName(sender: string): string {
  // decode RFC 2047 encoded-words (e.g. `=?UTF-8?B?...?=`) first so
  // both the "Name <email>" pattern and the raw-email fallback see
  // the real text, not the on-wire mime encoding
  const decoded = decodeMimeHeader(sender)
  const m = decoded.match(/^"?([^"<]+)"?\s*</)
  return m ? m[1].trim() : decoded.split('@')[0]
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  const value = bytes / Math.pow(1024, i)
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
}

export function formatRelative(ts: number): string {
  const now = Math.floor(Date.now() / 1000)
  const diff = now - ts
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  if (diff < 604800) return `${Math.floor(diff / 86400)}d ago`
  return new Date(ts * 1000).toLocaleDateString('en', {
    day: 'numeric',
    month: 'short',
  })
}

// Bucketed-width mapping: Tailwind can't generate w-[NN%] from a runtime
// number (purge happens at build), so we map to a small set of static
// arbitrary-value utilities. This keeps the CategoryBar bars in step with
// the theme without an inline style={width:...} (Tailwind-only rule).
export function pctToWidth(pct: number): string {
  if (pct >= 95) return 'w-full'
  if (pct >= 85) return 'w-[85%]'
  if (pct >= 75) return 'w-3/4'
  if (pct >= 65) return 'w-[65%]'
  if (pct >= 55) return 'w-[55%]'
  if (pct >= 45) return 'w-[45%]'
  if (pct >= 35) return 'w-[35%]'
  if (pct >= 25) return 'w-1/4'
  if (pct >= 15) return 'w-[15%]'
  if (pct >= 8) return 'w-[10%]'
  if (pct >= 3) return 'w-[5%]'
  if (pct > 0) return 'w-[3%]'
  return 'w-0'
}

export function todayStart(): number {
  const d = new Date()
  d.setHours(0, 0, 0, 0)
  return Math.floor(d.getTime() / 1000)
}

export function useGreeting() {
  const hour = new Date().getHours()
  if (hour < 6) return 'Good night'
  if (hour < 12) return 'Good morning'
  if (hour < 18) return 'Good afternoon'
  return 'Good evening'
}

export const COLOR_MAP = {
  brand: 'bg-accent/10 text-accent',
  danger: 'bg-red-500/10 text-red-500',
  info: 'bg-blue-500/10 text-blue-500',
  warning: 'bg-amber-500/10 text-amber-500',
} as const

export const CATEGORY_COLORS: Record<string, string> = {
  general: 'bg-gray-400',
  notification: 'bg-purple-500',
  personal: 'bg-blue-500',
  promotion: 'bg-amber-500',
  scam: 'bg-red-700',
  spam: 'bg-red-500',
}
