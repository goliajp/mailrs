import { toast } from '@goliapkg/gds'
import { useRef, useState } from 'react'

import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { invalidateMail } from '@/lib/mail-cache'
import { wireImportMbox } from '@/wire/endpoints/mail-import'
import { type WireMailImportReport } from '@/wire/schemas/mail-import'

import { btnPrimary, cardClass } from './_shared'

/** Upload an mbox (a Gmail Takeout, another client's export, or ours). */
export function MboxImportCard() {
  const t = useT()
  const input = useRef<HTMLInputElement>(null)
  const [busy, setBusy] = useState(false)
  const [report, setReport] = useState<null | WireMailImportReport>(null)

  const upload = async (file: File) => {
    setBusy(true)
    setReport(null)
    try {
      const r = await wireImportMbox(file)
      setReport(r)
      invalidateMail()
    } catch (e) {
      toast.error(errorText(e, t('settings.account.importUploadFailed')))
    } finally {
      setBusy(false)
      if (input.current) input.current.value = ''
    }
  }

  return (
    <div className={cardClass}>
      <h3 className="mb-2 text-sm font-medium">{t('settings.account.import')}</h3>
      <p className="text-fg-muted mb-3 text-xs">{t('settings.account.importHelp')}</p>
      <input
        accept=".mbox,application/mbox,text/plain"
        aria-label={t('settings.account.importFile')}
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) void upload(file)
        }}
        ref={input}
        type="file"
      />
      <button className={btnPrimary} disabled={busy} onClick={() => input.current?.click()}>
        {t(buttonLabel(busy))}
      </button>
      {report && <ImportReport report={report} />}
    </div>
  )
}

function buttonLabel(busy: boolean) {
  if (busy) return 'settings.account.importing' as const
  return 'settings.account.importMbox' as const
}

function ImportReport({ report }: { report: WireMailImportReport }) {
  const t = useT()
  return (
    <div className="text-fg-secondary mt-3 space-y-1 text-xs" role="status">
      <p>
        {t('settings.account.importDone', {
          archived: report.archived,
          imported: report.imported,
          sent: report.sent,
        })}
      </p>
      <p>
        {t('settings.account.importSkipped', {
          duplicate: report.skipped_duplicate,
          junk: report.skipped_junk_or_trash,
        })}
      </p>
      {report.too_large + report.failed > 0 && (
        <p className="text-red-400">
          {t('settings.account.importFailed', {
            failed: report.failed,
            tooLarge: report.too_large,
          })}
        </p>
      )}
      {!report.complete && <p className="text-red-400">{t('settings.account.importIncomplete')}</p>}
    </div>
  )
}
