import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { useEffect, useState } from 'react'

import { useSignOut } from '@/hooks/use-sign-out'
import { type Key, useT } from '@/i18n'
import { ensureConfirmed } from '@/lib/ensure-confirmed'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import { authAtom } from '@/store/auth'
import {
  wireChangePassword,
  wireGetRecoveryEmail,
  wireSetRecoveryEmail,
} from '@/wire/endpoints/auth'

import {
  btnDanger,
  btnPrimary,
  cardClass,
  ConfirmDialog,
  Field,
  inputClass,
  SectionHeader,
} from './_shared'
import { RevokeAllAppPasswords } from './app-passwords-card'
import { ImapImportCard } from './imap-import-card'
import { MboxImportCard } from './mbox-import-card'

export function AccountSection() {
  const t = useT()
  const auth = useAtomValue(authAtom)
  const [pw, setPw] = useState({ confirm: '', current: '', next: '' })
  const [saving, setSaving] = useState(false)
  const [recoveryEmail, setRecoveryEmail] = useState('')
  const [savingRecovery, setSavingRecovery] = useState(false)
  const [showLogoutConfirm, setShowLogoutConfirm] = useState(false)

  const recoveryQuery = useQuery({
    queryKey: settingsKeys.recoveryEmail(),
    queryFn: async () => {
      try {
        return await wireGetRecoveryEmail()
      } catch {
        return { recovery_email: '' }
      }
    },
  })
  const recoveryLoaded = !recoveryQuery.isLoading
  const [recoverySeeded, setRecoverySeeded] = useState(false)
  useEffect(() => {
    if (!recoverySeeded && recoveryQuery.isSuccess) {
      setRecoveryEmail(recoveryQuery.data.recovery_email)
      setRecoverySeeded(true)
    }
  }, [recoverySeeded, recoveryQuery.isSuccess, recoveryQuery.data])

  const handleRecoveryEmailSave = async () => {
    setSavingRecovery(true)
    try {
      await wireSetRecoveryEmail(recoveryEmail)
      toast.success(t('settings.account.recoveryUpdated'))
      void queryClient.invalidateQueries({ queryKey: settingsKeys.recoveryEmail() })
    } catch (e) {
      toast.error(errorText(e, t('settings.account.recoveryFailed')))
    } finally {
      setSavingRecovery(false)
    }
  }

  const handlePasswordChange = async () => {
    if (!pw.current || !pw.next) return
    if (pw.next !== pw.confirm) {
      toast.error(t('settings.account.mismatch'))
      return
    }
    if (pw.next.length < 12) {
      toast.error(t('reset.tooShort'))
      return
    }
    setSaving(true)
    try {
      await wireChangePassword(pw.current, pw.next)
      toast.success(t('settings.account.passwordUpdated'))
      setPw({ confirm: '', current: '', next: '' })
    } catch (e) {
      toast.error(errorText(e, t('settings.account.passwordFailed')))
    } finally {
      setSaving(false)
    }
  }

  const doLogout = useSignOut()

  // the export is a download, which never passes through `http`, so the
  // confirmation it needs is asked for first; an attachment leaves the page
  // where it is, and a new tab opened after the await would be blocked
  const exportMailbox = async () => {
    if (await ensureConfirmed()) window.location.assign('/api/mail/export')
  }

  return (
    <div className="space-y-6">
      <SectionHeader title={t('settings.cat.account')} />

      <div className={cardClass}>
        <div className="space-y-3">
          <Field label={t('login.email')}>
            <span className="text-fg-secondary text-sm">{auth?.address ?? '-'}</span>
          </Field>
          <Field label={t('settings.account.displayName')}>
            <span className="text-fg-secondary text-sm">{auth?.display_name || '-'}</span>
          </Field>
          <Field label={t('settings.account.permissions')}>
            <span className="text-fg-secondary text-sm break-all">
              {auth?.permissions?.join(', ') || '-'}
            </span>
          </Field>
          <Field label={t('settings.account.domains')}>
            <span className="text-fg-secondary text-sm break-all">
              {auth?.accessible_domains?.join(', ') || '-'}
            </span>
          </Field>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-3 text-sm font-medium">{t('login.recoveryEmail')}</h3>
        <p className="text-fg-muted mb-2 text-xs">{t('settings.account.recoveryHelp')}</p>
        <div className="flex gap-2">
          <input
            className={inputClass + ' flex-1'}
            disabled={!recoveryLoaded}
            onChange={(e) => setRecoveryEmail(e.target.value)}
            placeholder={t(recoveryPlaceholder(recoveryLoaded))}
            type="email"
            value={recoveryEmail}
          />
          <button
            className={btnPrimary}
            disabled={savingRecovery || !recoveryLoaded}
            onClick={handleRecoveryEmailSave}
          >
            {t(saveLabel(savingRecovery, 'settings.save'))}
          </button>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-3 text-sm font-medium">{t('settings.account.changePassword')}</h3>
        <div className="space-y-2">
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-current-pw"
            >
              {t('settings.account.currentPassword')}
            </label>
            <input
              autoComplete="current-password"
              className={inputClass}
              id="settings-current-pw"
              onChange={(e) => setPw({ ...pw, current: e.target.value })}
              placeholder={t('settings.account.currentPassword')}
              type="password"
              value={pw.current}
            />
          </div>
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-new-pw"
            >
              {t('reset.newPassword')}
            </label>
            <input
              autoComplete="new-password"
              className={inputClass}
              id="settings-new-pw"
              onChange={(e) => setPw({ ...pw, next: e.target.value })}
              placeholder={t('reset.newPassword')}
              type="password"
              value={pw.next}
            />
          </div>
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-confirm-pw"
            >
              {t('settings.account.confirmNew')}
            </label>
            <input
              autoComplete="new-password"
              className={inputClass}
              id="settings-confirm-pw"
              onChange={(e) => setPw({ ...pw, confirm: e.target.value })}
              placeholder={t('settings.account.confirmNew')}
              type="password"
              value={pw.confirm}
            />
          </div>
          <button className={btnPrimary} disabled={saving} onClick={handlePasswordChange}>
            {t(saveLabel(saving, 'settings.account.updatePassword'))}
          </button>
          <RevokeAllAppPasswords />
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-2 text-sm font-medium">{t('settings.account.export')}</h3>
        <p className="text-fg-muted mb-3 text-xs">{t('settings.account.exportHelp')}</p>
        <button className={btnPrimary} onClick={() => void exportMailbox()}>
          {t('settings.account.exportMbox')}
        </button>
      </div>

      <MboxImportCard />

      <ImapImportCard />

      <div className="border-border border-t pt-4">
        <button className={btnDanger} onClick={() => setShowLogoutConfirm(true)}>
          {t('settings.account.signOut')}
        </button>
        {auth?.address && (
          <p className="text-fg-muted mt-2 text-xs">
            {t('settings.account.signedInAs', { address: auth.address })}
          </p>
        )}
      </div>

      {showLogoutConfirm && (
        <ConfirmDialog
          message={t('settings.account.signOutConfirm')}
          onCancel={() => setShowLogoutConfirm(false)}
          onConfirm={doLogout}
        />
      )}
    </div>
  )
}

function recoveryPlaceholder(loaded: boolean): Key {
  if (loaded) return 'settings.account.notConfigured'
  return 'settings.loadingDots'
}

function saveLabel(saving: boolean, idle: Key): Key {
  if (saving) return 'settings.saving'
  return idle
}
