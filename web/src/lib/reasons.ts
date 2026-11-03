/**
 * Why a message was judged as it was, in the reader's language.
 *
 * The server stores a reason as a code and the values its sentence
 * names; the words come from `reasons.json`, a byte-identical copy of
 * crates/inbound/src/reasons.json (a Rust test compares them).
 */
import type { Lang, Translate } from '@/i18n'
import type { WireReason } from '@/wire/schemas/reason'

import table from './reasons.json'

/**
 * A code's sentence, and for a rule that names facts, a second sentence
 * that says them: used when the reason carries every value it names.
 */
type Entry = Record<Lang, string> & { params?: Record<Lang, string> }

const TABLE: Record<string, Entry | undefined> = table

/** The category names a `class` param can carry, as the reader knows them. */
const CLASS_KEY = {
  inbox: 'reason.class.inbox',
  notification: 'reason.class.notification',
  promotion: 'reason.class.promotion',
} as const

/** Whether the table has a sentence for `code`. */
export function hasReasonText(code: string): boolean {
  return TABLE[code] !== undefined
}

/**
 * The sentence for `reason`, or `null` when this client has none for its
 * code: a rule added on the server after this build. The caller then
 * shows the server's own English detail.
 */
export function reasonText(reason: WireReason, lang: Lang, t: Translate): null | string {
  const entry = TABLE[reason.code]
  if (!entry) return null
  const params = reason.params ?? {}
  return sentenceFor(entry, lang, params).replace(/\{(\w+)\}/g, (whole, name: string) => {
    const v = params[name]
    if (v === undefined) return whole
    if (name === 'class') return className(v, t)
    return v
  })
}

function className(value: string, t: Translate): string {
  if (value in CLASS_KEY) return t(CLASS_KEY[value as keyof typeof CLASS_KEY])
  return value
}

/**
 * The sentence with the facts when the reason carries all of them; a
 * verdict stored before the rule named them gets the general sentence.
 */
function sentenceFor(entry: Entry, lang: Lang, params: Record<string, string>): string {
  const withParams = entry.params?.[lang]
  if (withParams === undefined) return entry[lang]
  const names = [...withParams.matchAll(/\{(\w+)\}/g)].map((m) => m[1])
  if (names.every((n) => params[n] !== undefined)) return withParams
  return entry[lang]
}
