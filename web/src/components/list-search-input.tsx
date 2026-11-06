import type { ReactNode } from 'react'

import { useAtom } from 'jotai'
import { Search, X } from 'lucide-react'
import { useId, useState } from 'react'

import { useT } from '@/i18n'
import { searchQueryAtom } from '@/store/ui'

/**
 * The search header every mail list draws.
 *
 * It owns `searchQueryAtom` rather than taking a value and a setter:
 * one question per screen, and a caller cannot wire it to the wrong
 * one. Draft and Send each had a private atom until 2026-08-05, so the
 * same box asked something different depending on which chip was lit
 * and whatever you had typed vanished when you changed tab.
 *
 * `children` are the trailing controls of the row — batch select, mark
 * all read, compose. Only the conversation lists have any; they used to
 * be the reason that list hand-rolled a byte-identical copy of this
 * markup instead of using it.
 *
 * `hint` is a line under the empty box while it has focus. The search
 * operators used to be the placeholder, which a list column cut off after
 * three of them; iOS shows them the same way, under the field.
 */
export function ListSearchInput({
  children,
  hint,
  label,
  placeholder,
}: {
  children?: ReactNode
  hint?: string
  label: string
  placeholder?: string
}) {
  const t = useT()
  const hintId = useId()
  const [value, setValue] = useAtom(searchQueryAtom)
  const [focused, setFocused] = useState(false)
  const active = value.trim().length > 0
  const showHint = !!hint && focused && value === ''
  return (
    <div className="border-border border-b px-3 py-2">
      <div className="flex items-center gap-2">
        <div className="relative flex-1" role="search">
          <Search
            aria-hidden="true"
            className="text-fg-muted absolute top-1/2 left-2.5 h-4 w-4 -translate-y-1/2"
          />
          <input
            aria-describedby={hintIdOf(showHint, hintId)}
            aria-label={label}
            className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:bg-bg w-full rounded-md border py-2 pr-8 pl-9 text-sm transition-colors outline-none"
            onBlur={() => setFocused(false)}
            onChange={(e) => setValue(e.target.value)}
            onFocus={() => setFocused(true)}
            placeholder={placeholder ?? t('search.placeholder')}
            type="text"
            value={value}
          />
          {active && (
            <button
              aria-label={t('search.clear')}
              className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary focus-visible:ring-accent/50 absolute top-1/2 right-1.5 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-md transition-colors focus-visible:ring-2 focus-visible:outline-none"
              onClick={() => setValue('')}
              type="button"
            >
              <X className="h-4 w-4" />
            </button>
          )}
        </div>
        {children}
      </div>
      {showHint && (
        <p className="text-fg-muted mt-1.5 px-1 text-xs break-words" id={hintId}>
          {hint}
        </p>
      )}
    </div>
  )
}

function hintIdOf(shown: boolean, id: string): string | undefined {
  if (shown) return id
  return undefined
}
