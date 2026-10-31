import { Loader2, Sparkles } from 'lucide-react'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { useT } from '@/i18n'

type AddressFieldsProps = {
  bcc: string
  cc: string
  generatingSubject: boolean
  onBccChange: (v: string) => void
  onCcChange: (v: string) => void
  onGenerateSubject: () => void
  onShowCcBcc: () => void
  onSubjectChange: (v: string) => void
  onToChange: (v: string) => void
  sending: boolean
  showCcBcc: boolean
  subject: string
  to: string
}

export function AddressFields({
  bcc,
  cc,
  generatingSubject,
  onBccChange,
  onCcChange,
  onGenerateSubject,
  onShowCcBcc,
  onSubjectChange,
  onToChange,
  sending,
  showCcBcc,
  subject,
  to,
}: AddressFieldsProps) {
  const t = useT()
  return (
    <div className="border-border flex shrink-0 flex-col border-b">
      <div className="border-border flex h-9 items-center border-b px-4">
        <span className="text-fg-muted w-14 shrink-0 text-xs">{t('compose.to')}</span>
        <ContactAutocomplete
          autoFocus
          onChange={onToChange}
          placeholder="recipient@example.com"
          value={to}
        />
        {!showCcBcc && (
          <button
            className="text-fg-muted hover:text-fg-secondary shrink-0 text-xs transition-colors"
            onClick={onShowCcBcc}
            type="button"
          >
            {t('compose.ccBcc')}
          </button>
        )}
      </div>
      {showCcBcc && (
        <>
          <div className="border-border flex h-9 items-center border-b px-4">
            <span className="text-fg-muted w-14 shrink-0 text-xs">{t('compose.cc')}</span>
            <ContactAutocomplete onChange={onCcChange} placeholder="cc@example.com" value={cc} />
          </div>
          <div className="border-border flex h-9 items-center border-b px-4">
            <span className="text-fg-muted w-14 shrink-0 text-xs">{t('compose.bcc')}</span>
            <ContactAutocomplete onChange={onBccChange} placeholder="bcc@example.com" value={bcc} />
          </div>
        </>
      )}
      <div className="border-border flex h-9 items-center border-b px-4">
        <label className="text-fg-muted w-14 shrink-0 text-xs" htmlFor="new-conv-subject">
          {t('compose.subject')}
        </label>
        <input
          className="text-fg flex-1 bg-transparent py-2 text-sm outline-none"
          id="new-conv-subject"
          onChange={(e) => onSubjectChange(e.target.value)}
          type="text"
          value={subject}
        />
        <button
          aria-label={t('compose.aiSubject')}
          className="text-fg-muted hover:bg-accent/10 hover:text-accent shrink-0 rounded-md p-1 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          disabled={generatingSubject || sending}
          onClick={onGenerateSubject}
          title={t('compose.aiSubject')}
          type="button"
        >
          {generatingSubject ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Sparkles className="h-3.5 w-3.5" />
          )}
        </button>
      </div>
    </div>
  )
}
