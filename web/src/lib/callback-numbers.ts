// Phone numbers a message asks the reader to call, and which of them the
// same sender has not written before in this conversation.
//
// A number proves nothing about who answers it, so this only points it
// out; it never judges the message. Pure, so it unit-tests as .test.ts.
// The labels and the samples it is held to are `callback-numbers.json`,
// which iOS and Android test their own copies of this rule against.

import table from './callback-numbers.json'

type Sourced = { html_body: null | string; sender: string; text_body: null | string }

// One phone number, by its structure: an optional `+` and country code,
// an optional bracketed area code, then digit groups of at most six joined
// by one space, dot or hyphen each. A digit, letter or hyphen on either
// side means the digits belong to something else (2026-INV-003001-2), as
// do a currency sign, a thousands comma or a decimal point before them.
// A line break is not a separator, so numbers on two lines never join.
const SEP = '[ \\u00A0.\\-‐－]'
const CANDIDATE = new RegExp(
  '(?<![A-Za-z0-9_+.,/#\\x24€£¥\\-‐－])' +
    `(?:\\+\\d{1,3}${SEP}?)?(?:\\(\\d{1,4}\\)${SEP}?)?\\d{1,6}(?:${SEP}\\d{1,6}){1,5}` +
    '(?![A-Za-z0-9_:\\-‐－]|[.,/]\\d)',
  'g'
)
// dates and times written with dots, dashes or spaces: 2026-11-01 12.30
const DATE_LIKE = /^\d{4}[-./ ‐－]\d{1,2}[-./ ‐－]\d{1,2}(?!\d)/
// 1 234 567 890: an amount grouped by thousands, not a number to dial
const THOUSANDS = /^[1-9]\d{0,2}(?:[ \u00A0.]\d{3})+$/
// digits straight after one of these name an order, invoice or account
const AFTER_LABEL = new RegExp(
  `(?:^|[^A-Za-z0-9])(?:${[...table.labels]
    .sort((a, b) => b.length - a.length)
    .map(escapeLabel)
    .join('|')})[\\s:：.#＃№\\-]*$`,
  'i'
)
const LABEL_WINDOW = 40
const MIN_DIGITS = 10
// a national number with no country code is at most 11 digits in the
// numbering plans this has to read (JP, US, UK)
const MAX_NATIONAL_DIGITS = 11
const MAX_DIGITS = 15
const MAX_SHOWN = 3

/**
 * Numbers in `message` that no earlier message from the same address in
 * `earlier` carried: a number a correspondent always signs with is not
 * worth pointing out, a new one asking to be called is.
 */
export function newCallbackNumbers(message: Sourced, earlier: readonly Sourced[]): string[] {
  const own = message.sender.toLowerCase()
  const before = new Set(
    earlier
      .filter((e) => e.sender.toLowerCase() === own)
      .flatMap((e) => phoneNumbers(plainText(e)).map(digits))
  )
  return phoneNumbers(plainText(message))
    .filter((n) => !before.has(digits(n)))
    .slice(0, MAX_SHOWN)
}

/**
 * Numbers in `text` that read as phone numbers, as written, once each.
 * Misses some real ones on purpose: the notice is worth reading only
 * while it is almost never wrong.
 */
export function phoneNumbers(text: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const match of text.matchAll(CANDIDATE)) {
    const written = match[0]
    const d = digits(written)
    if (seen.has(d) || !dialable(written, d)) continue
    if (AFTER_LABEL.test(text.slice(Math.max(0, match.index - LABEL_WINDOW), match.index))) continue
    seen.add(d)
    out.push(written)
  }
  return out
}

function dialable(written: string, d: string): boolean {
  if (d.length < MIN_DIGITS || d.length > maxDigits(written)) return false
  return !DATE_LIKE.test(written) && !THOUSANDS.test(written)
}

function digits(s: string): string {
  return s.replace(/\D/g, '')
}

function escapeLabel(label: string): string {
  return label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function maxDigits(written: string): number {
  if (written.startsWith('+')) return MAX_DIGITS
  return MAX_NATIONAL_DIGITS
}

function plainText(m: Sourced): string {
  if (m.text_body) return m.text_body
  if (!m.html_body) return ''
  return new DOMParser().parseFromString(m.html_body, 'text/html').body.textContent ?? ''
}
