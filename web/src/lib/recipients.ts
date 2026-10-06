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
  return splitAddressList(value).map((entry) => {
    const address = extractEmail(entry)
    const name = extractName(entry)
    return { address: address || entry, name: name || address || entry }
  })
}

/** The entries of an address list as written, trimmed, empties dropped. */
export function splitAddressList(value: string): string[] {
  const out: string[] = []
  let quoted = false
  let current = ''
  const push = (raw: string) => {
    const trimmed = raw.trim()
    if (trimmed) out.push(trimmed)
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
