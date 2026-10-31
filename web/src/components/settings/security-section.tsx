import type { TotpSetup } from './_shared'
import type { Key } from '@/i18n'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useT } from '@/i18n'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireGetTotpStatus,
  wireTotpDisable,
  wireTotpEnable,
  wireTotpSetup,
} from '@/wire/endpoints/auth'

import { btnDanger, btnPrimary, cardClass, Field, inputClass, SectionHeader } from './_shared'
import { AppPasswordsCard } from './app-passwords-card'
import { SessionsCard } from './sessions-card'

export function SecuritySection() {
  const t = useT()
  const statusQuery = useQuery({
    queryKey: settingsKeys.totpStatus(),
    queryFn: () => wireGetTotpStatus(),
  })
  const status = statusQuery.data ?? null
  const loading = statusQuery.isLoading
  const [setup, setSetup] = useState<null | TotpSetup>(null)
  const [code, setCode] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const invalidateStatus = () =>
    queryClient.invalidateQueries({ queryKey: settingsKeys.totpStatus() })

  const handleSetup = async () => {
    try {
      const data = await wireTotpSetup()
      setSetup(data)
    } catch (e) {
      toast.error(errorText(e, t('security.totp.setupFailed')))
    }
  }

  const handleEnable = async () => {
    if (!code.trim()) return
    setSubmitting(true)
    try {
      await wireTotpEnable(code.trim())
      toast.success(t('security.totp.enabledToast'))
      setSetup(null)
      setCode('')
      void invalidateStatus()
    } catch (e) {
      toast.error(errorText(e, t('security.totp.invalidCode')))
    } finally {
      setSubmitting(false)
    }
  }

  const handleDisable = async () => {
    if (!code.trim()) return
    setSubmitting(true)
    try {
      await wireTotpDisable(code.trim())
      toast.success(t('security.totp.disabledToast'))
      setCode('')
      void invalidateStatus()
    } catch (e) {
      toast.error(errorText(e, t('security.totp.invalidCode')))
    } finally {
      setSubmitting(false)
    }
  }

  if (loading) {
    return (
      <div className="text-fg-muted flex items-center gap-2 py-4 text-sm">
        <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
        {t('common.loading')}
      </div>
    )
  }

  const enabled = Boolean(status?.enabled)
  return (
    <div className="space-y-6">
      <SectionHeader title={t('security.totp.title')} />

      <div className={cardClass}>
        <Field label={t('security.totp.status')}>
          <span className={statusClass(enabled)}>
            {t(pick(enabled, 'security.totp.enabled', 'security.totp.disabled'))}
          </span>
        </Field>
      </div>

      {!enabled && !setup && (
        <button className={btnPrimary} onClick={handleSetup} type="button">
          {t('security.totp.setUp')}
        </button>
      )}

      {setup && (
        <div className={cardClass + ' space-y-4'}>
          <h3 className="text-sm font-medium">{t('security.totp.scan')}</h3>
          <div className="bg-surface rounded-md p-3">
            <p className="text-fg-secondary font-mono text-xs break-all">{setup.otpauth_url}</p>
          </div>

          {setup.recovery_codes.length > 0 && (
            <div className="border-warning bg-warning/10 rounded-lg border p-3">
              <p className="mb-2 text-sm font-semibold">{t('security.totp.recoveryCodes')}</p>
              <p className="text-fg-secondary mb-2 text-xs">
                {t('security.totp.recoveryCodesNote')}
              </p>
              <div className="grid grid-cols-2 gap-1">
                {setup.recovery_codes.map((rc) => (
                  <code className="bg-bg-secondary rounded px-2 py-1 font-mono text-xs" key={rc}>
                    {rc}
                  </code>
                ))}
              </div>
            </div>
          )}

          <div className="flex gap-2">
            <input
              aria-label={t('security.totp.code')}
              autoComplete="one-time-code"
              className={inputClass + ' max-w-[200px]'}
              inputMode="numeric"
              onChange={(e) => setCode(e.target.value)}
              placeholder={t('security.totp.codePlaceholder')}
              value={code}
            />
            <button
              className={btnPrimary}
              disabled={submitting}
              onClick={handleEnable}
              type="button"
            >
              {t(pick(submitting, 'security.totp.verifying', 'security.totp.verifyEnable'))}
            </button>
          </div>
        </div>
      )}

      {enabled && (
        <div className={cardClass + ' space-y-3'}>
          <h3 className="text-sm font-medium">{t('security.totp.disableTitle')}</h3>
          <div className="flex gap-2">
            <input
              aria-label={t('security.totp.code')}
              autoComplete="one-time-code"
              className={inputClass + ' max-w-[200px]'}
              inputMode="numeric"
              onChange={(e) => setCode(e.target.value)}
              placeholder={t('security.totp.codePlaceholder')}
              value={code}
            />
            <button
              className={btnDanger}
              disabled={submitting}
              onClick={handleDisable}
              type="button"
            >
              {t(pick(submitting, 'security.totp.disabling', 'security.totp.disableTitle'))}
            </button>
          </div>
        </div>
      )}
      <AppPasswordsCard />
      <SessionsCard />
    </div>
  )
}

/** The server's message when there is one, else ours. */
function errorText(e: unknown, fallback: string): string {
  if (e instanceof Error) return e.message
  return fallback
}

function pick(busy: boolean, busyKey: Key, idleKey: Key): Key {
  if (busy) return busyKey
  return idleKey
}

function statusClass(enabled: boolean): string {
  const base = 'rounded-full px-2 py-0.5 text-xs font-medium'
  if (enabled) return `${base} bg-success/10 text-success`
  return `${base} bg-surface text-fg-muted`
}
