/**
 * The interface language and `t`, its lookup. The reader's choice when
 * they made one, otherwise the browser's; kept in localStorage, a personal
 * preference.
 */

import { atom, getDefaultStore, useAtomValue } from 'jotai'
import { useCallback } from 'react'

import { en, type Key } from './en'
import { ja } from './ja'
import { zh } from './zh'

export type { Key } from './en'

export type Lang = 'en' | 'ja' | 'zh'

export const LANGS: Lang[] = ['en', 'ja', 'zh']

const DICTS: Record<Lang, Record<Key, string>> = { en, ja, zh }

const STORAGE_KEY = 'mailrs_lang'

/** The browser's language, as one of ours: Chinese, Japanese, else English. */
export function browserLang(language: string): Lang {
  const l = language.toLowerCase()
  if (l.startsWith('zh')) return 'zh'
  if (l.startsWith('ja')) return 'ja'
  return 'en'
}

function stored(): Lang | null {
  const v = localStorage.getItem(STORAGE_KEY)
  if (v === 'en' || v === 'ja' || v === 'zh') return v
  return null
}

const chosenAtom = atom<Lang | null>(stored())

/** What the reader chose; `null` follows the browser. */
export const chosenLangAtom = atom(
  (get) => get(chosenAtom),
  (_get, set, value: Lang | null) => {
    if (value === null) localStorage.removeItem(STORAGE_KEY)
    else localStorage.setItem(STORAGE_KEY, value)
    set(chosenAtom, value)
  }
)

export const langAtom = atom<Lang>((get) => get(chosenAtom) ?? browserLang(navigator.language))

/** Keep `<html lang>` on the interface language, so screen readers and
 * the browser read the page in it. Called once at start. */
export function syncDocumentLang() {
  const store = getDefaultStore()
  const apply = () => {
    document.documentElement.lang = store.get(langAtom)
  }
  apply()
  store.sub(langAtom, apply)
}

/** `key` in `lang`, with each `{name}` replaced by `vars[name]`. */
export function translate(lang: Lang, key: Key, vars?: Record<string, number | string>): string {
  const text = DICTS[lang][key]
  if (!vars) return text
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const v = vars[name]
    if (v === undefined) return whole
    return String(v)
  })
}

/** `t` for the current language. */
export function useT() {
  const lang = useAtomValue(langAtom)
  return useCallback(
    (key: Key, vars?: Record<string, number | string>) => translate(lang, key, vars),
    [lang]
  )
}
