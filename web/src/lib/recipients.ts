import { extractEmail, extractName } from '@/lib/avatar'

/**
 * One entry per addressee, keeping both halves.
 *
 * Split on commas **outside quotes**: a display name is allowed to
 * contain one — `"Lastname, Firstname" <x@y>` is ordinary in corporate
 * mail — and splitting naively turns one person into two, the second
 * of whom has no address at all.
 */
export function splitAddresses(value: string): { address: string; name: string }[] {
  const out: { address: string; name: string }[] = []
  let quoted = false
  let current = ''
  const push = (raw: string) => {
    const trimmed = raw.trim()
    if (!trimmed) return
    const address = extractEmail(trimmed)
    const name = extractName(trimmed)
    out.push({ address: address || trimmed, name: name || address || trimmed })
  }
  for (const ch of value) {
    if (ch === '"') {
      quoted = !quoted
      current += ch
      continue
    }
    if (ch === ',' && !quoted) {
      push(current)
      current = ''
      continue
    }
    current += ch
  }
  push(current)
  return out
}
