import type { Composer } from './use-composer'
import type { ComposerAi } from './use-composer-ai'

import { Loader2, Sparkles } from 'lucide-react'
import { useId } from 'react'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { useT } from '@/i18n'

const ROW = 'border-border flex min-h-9 items-center border-b px-4'
const LABEL = 'text-fg-muted w-14 shrink-0 text-xs'
const INPUT = 'text-fg min-w-0 flex-1 bg-transparent py-2 text-sm outline-none'

/** To, Cc/Bcc, From and Subject: the same fields in every shell. */
export function ComposerFields({
  ai,
  c,
  focusTo,
}: {
  ai: ComposerAi
  c: Composer
  /** Put the caret in To when the fields appear. */
  focusTo: boolean
}) {
  const t = useT()
  const id = useId()
  return (
    <div className="border-border flex shrink-0 flex-col border-b">
      <div className={ROW}>
        <label className={LABEL} htmlFor={`${id}-to`}>
          {t('compose.to')}
        </label>
        <ContactAutocomplete
          autoFocus={focusTo}
          id={`${id}-to`}
          onChange={c.setTo}
          placeholder="recipient@example.com"
          value={c.to}
        />
        {!c.showCcBcc && (
          <button
            className="text-fg-muted hover:text-fg-secondary shrink-0 text-xs transition-colors"
            onClick={() => c.setShowCcBcc(true)}
            type="button"
          >
            {t('compose.ccBcc')}
          </button>
        )}
      </div>
      {c.showCcBcc && (
        <>
          <div className={ROW}>
            <label className={LABEL} htmlFor={`${id}-cc`}>
              {t('compose.cc')}
            </label>
            <ContactAutocomplete
              id={`${id}-cc`}
              onChange={c.setCc}
              placeholder="cc@example.com"
              value={c.cc}
            />
          </div>
          <div className={ROW}>
            <label className={LABEL} htmlFor={`${id}-bcc`}>
              {t('compose.bcc')}
            </label>
            <ContactAutocomplete
              id={`${id}-bcc`}
              onChange={c.setBcc}
              placeholder="bcc@example.com"
              value={c.bcc}
            />
          </div>
        </>
      )}
      {c.fromOptions.length > 1 && (
        <div className={ROW}>
          <label className={LABEL} htmlFor={`${id}-from`}>
            {t('composer.from')}
          </label>
          <select
            className={INPUT}
            id={`${id}-from`}
            onChange={(e) => c.setFrom(e.target.value)}
            value={c.from}
          >
            {c.fromOptions.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </select>
        </div>
      )}
      <div className={ROW}>
        <label className={LABEL} htmlFor={`${id}-subject`}>
          {t('compose.subject')}
        </label>
        <input
          className={INPUT}
          id={`${id}-subject`}
          onChange={(e) => c.setSubject(e.target.value)}
          type="text"
          value={c.subject}
        />
        <button
          aria-label={t('compose.aiSubject')}
          className="text-fg-muted hover:bg-accent/10 hover:text-accent shrink-0 rounded-md p-1 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          disabled={ai.generatingSubject || c.sending}
          onClick={() => void ai.generateSubject()}
          title={t('compose.aiSubject')}
          type="button"
        >
          {ai.generatingSubject && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {!ai.generatingSubject && <Sparkles className="h-3.5 w-3.5" />}
        </button>
      </div>
    </div>
  )
}
