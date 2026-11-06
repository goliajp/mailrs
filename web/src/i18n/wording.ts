/**
 * The shared wording table: what the three clients call the inbox, each
 * list, the invitation answers and the rest, in each language. The phones
 * ship the same file and keep their catalogs equal to it
 * (`scripts/check-wording.sh`); the dictionaries here take those entries
 * from it with `word`, so the web cannot drift from it either.
 */

import table from '@/lib/wording.json'

type Lang = 'en' | 'ja' | 'zh'

const BY_KEY = new Map<string, Record<Lang, string>>(
  table.terms.flatMap((term) => term.web.map((key) => [key, term.text] as const))
)

/** The table's text for the web key `key`, in `lang`. */
export function word(lang: Lang, key: string): string {
  const text = BY_KEY.get(key)
  if (text === undefined) throw new Error(`${key} is not in wording.json`)
  return text[lang]
}
