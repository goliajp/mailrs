import { useAtomValue } from 'jotai'
import { useState } from 'react'

import { MobileModal } from '@/components/mobile-modal'
import { ResetCodePanel } from '@/components/reset-code-panel'
import { btnSecondary } from '@/components/settings/_shared'
import { useT } from '@/i18n'
import { authAtom } from '@/store/auth'

/**
 * The way to a reset code from where an administrator meets a cleared
 * account. Shown to issuers only: the server refuses anyone else.
 */
export function ResetCodeButton({ address }: { address: string }) {
  const t = useT()
  const issuer = useAtomValue(authAtom)?.is_reset_issuer === true
  const [open, setOpen] = useState(false)
  if (!issuer) return null
  return (
    <>
      <button
        className="text-accent text-xs whitespace-nowrap hover:opacity-80"
        onClick={() => setOpen(true)}
        type="button"
      >
        {t('settings.resetCodes.issue')}
      </button>
      {open && <ResetCodeDialog address={address} onClose={() => setOpen(false)} />}
    </>
  )
}

/** A reset code for an account whose password was cleared. */
export function ResetCodeDialog({ address, onClose }: { address: string; onClose: () => void }) {
  const t = useT()
  return (
    <MobileModal onClose={onClose} open>
      <div className="bg-surface flex w-full max-w-lg flex-col gap-4 rounded-lg p-6 shadow-lg">
        <h2 className="text-base font-semibold break-all">
          {t('admin.resetCode.title', { address })}
        </h2>
        <p className="text-fg-secondary text-sm">{t('settings.resetCodes.help')}</p>
        <ResetCodePanel address={address} />
        <div className="flex justify-end">
          <button className={btnSecondary} onClick={onClose} type="button">
            {t('common.close')}
          </button>
        </div>
      </div>
    </MobileModal>
  )
}
