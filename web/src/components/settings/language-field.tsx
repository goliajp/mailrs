import type { Lang } from '@/i18n'

import { useAtom } from 'jotai'

import { chosenLangAtom, LANGS, useT } from '@/i18n'

import { Field } from './_shared'

const NAME: Record<Lang, 'lang.name.en' | 'lang.name.ja' | 'lang.name.zh'> = {
  en: 'lang.name.en',
  ja: 'lang.name.ja',
  zh: 'lang.name.zh',
}

/** The interface language: the browser's, or one chosen here. */
export function LanguageField() {
  const t = useT()
  const [chosen, setChosen] = useAtom(chosenLangAtom)
  return (
    <Field label={t('lang.setting')}>
      <div className="flex flex-wrap gap-1">
        <button
          aria-pressed={chosen === null}
          className={optionClass(chosen === null)}
          onClick={() => setChosen(null)}
          type="button"
        >
          {t('lang.setting.browser')}
        </button>
        {LANGS.map((l) => (
          <button
            aria-pressed={chosen === l}
            className={optionClass(chosen === l)}
            key={l}
            lang={l}
            onClick={() => setChosen(l)}
            type="button"
          >
            {t(NAME[l])}
          </button>
        ))}
      </div>
    </Field>
  )
}

function optionClass(selected: boolean): string {
  const base = 'rounded-md px-3 py-1.5 text-sm transition-colors'
  if (selected) return `${base} bg-accent text-accent-fg`
  return `${base} bg-surface text-fg-secondary hover:bg-bg-secondary`
}
