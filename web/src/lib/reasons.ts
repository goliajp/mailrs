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

const TABLE: Record<string, Record<Lang, string> | undefined> = table

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
  return entry[lang].replace(/\{(\w+)\}/g, (whole, name: string) => {
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
