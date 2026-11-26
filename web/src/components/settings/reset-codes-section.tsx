import { useState } from 'react'

import { ResetCodePanel } from '@/components/reset-code-panel'
import { btnSecondary, cardClass, inputClass, SectionHeader } from '@/components/settings/_shared'
import { useT } from '@/i18n'

/**
 * Handing out reset codes: for the accounts whose password an
 * administrator cleared.
 */
export function ResetCodesSection() {
  const t = useT()
  const [draft, setDraft] = useState('')
  const [address, setAddress] = useState('')

  const lookUp = (e: React.FormEvent) => {
    e.preventDefault()
    setAddress(draft.trim().toLowerCase())
  }

  return (
    <div className="space-y-4">
      <SectionHeader title={t('settings.cat.reset-codes')} />
      <p className="text-fg-secondary text-sm">{t('settings.resetCodes.help')}</p>

      <form className="flex gap-2" onSubmit={lookUp}>
        <input
          aria-label={t('settings.resetCodes.account')}
          className={inputClass}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="someone@golia.jp"
          type="email"
          value={draft}
        />
        <button
          className={`${btnSecondary} shrink-0 whitespace-nowrap`}
          disabled={!draft.trim()}
          type="submit"
        >
          {t('settings.resetCodes.lookUp')}
        </button>
      </form>

      {address !== '' && (
        <div className={cardClass}>
          <p className="mb-2 text-sm font-medium break-all">{address}</p>
          {/* keyed: a code issued for one account is not shown under another */}
          <ResetCodePanel address={address} key={address} />
        </div>
      )}
    </div>
  )
}
