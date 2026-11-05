import { extractEmail, extractName } from '@/lib/avatar'

export type Person = { address: string; name: string }

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

/**
 * Above this many addressees (to and cc together) the header shows one
 * summary line instead of every name. Eight short names already fill
 * about two lines of a reading pane; a list longer than that is a
 * mailing, and the names past the first few are not what the reader
 * opened the message for.
 */
export const COLLAPSE_ABOVE = 8

/** How many addressees besides the reader the summary line names. */
const NAMED_IN_SUMMARY = 3

export type RecipientSummary = {
  /** The reader is among the to / cc addressees (not true of a Bcc). */
  me: boolean
  named: Person[]
  /** Addressees neither named nor the reader. */
  rest: number
  total: number
}

/**
 * The summary line for a long list, or `null` when the list is short
 * enough to show whole. The reader goes first when they are on it.
 */
export function summarizeRecipients(
  to: Person[],
  cc: Person[],
  myEmail: string
): null | RecipientSummary {
  const total = to.length + cc.length
  if (total <= COLLAPSE_ABOVE) return null
  const mine = myEmail.trim().toLowerCase()
  let mineCount = 0
  const named: Person[] = []
  for (const person of [...to, ...cc]) {
    if (mine && person.address.toLowerCase() === mine) {
      mineCount += 1
      continue
    }
    if (named.length < NAMED_IN_SUMMARY) named.push(person)
  }
  return { me: mineCount > 0, named, rest: total - mineCount - named.length, total }
}
