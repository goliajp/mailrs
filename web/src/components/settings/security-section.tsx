import type { TotpSetup } from './_shared'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireGetTotpStatus,
  wireTotpDisable,
  wireTotpEnable,
  wireTotpSetup,
} from '@/wire/endpoints/auth'

import { btnDanger, btnPrimary, cardClass, Field, inputClass, SectionHeader } from './_shared'

export function SecuritySection() {
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
      toast.error(e instanceof Error ? e.message : 'Failed to set up 2FA')
    }
  }

  const handleEnable = async () => {
    if (!code.trim()) return
    setSubmitting(true)
    try {
      await wireTotpEnable(code.trim())
      toast.success('2FA enabled')
      setSetup(null)
      setCode('')
      void invalidateStatus()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Invalid code')
    } finally {
      setSubmitting(false)
    }
  }

  const handleDisable = async () => {
    if (!code.trim()) return
    setSubmitting(true)
    try {
      await wireTotpDisable(code.trim())
      toast.success('2FA disabled')
      setCode('')
      void invalidateStatus()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Invalid code')
    } finally {
      setSubmitting(false)
    }
  }

  if (loading) {
    return (
      <div className="text-fg-muted flex items-center gap-2 py-4 text-sm">
        <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
        Loading...
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <SectionHeader title="Two-Factor Authentication" />

      <div className={cardClass}>
        <Field label="Status">
          <span
            className={`rounded-full px-2 py-0.5 text-xs font-medium ${
              status?.enabled ? 'bg-success/10 text-success' : 'bg-surface text-fg-muted'
            }`}
          >
            {status?.enabled ? 'Enabled' : 'Disabled'}
          </span>
        </Field>
      </div>

      {!status?.enabled && !setup && (
        <button className={btnPrimary} onClick={handleSetup}>
          Set up 2FA
        </button>
      )}

      {setup && (
        <div className={cardClass + ' space-y-4'}>
          <h3 className="text-sm font-medium">Scan QR Code</h3>
          <div className="bg-surface rounded-md p-3">
            <p className="text-fg-secondary font-mono text-xs break-all">{setup.otpauth_url}</p>
          </div>

          {setup.recovery_codes.length > 0 && (
            <div className="border-warning bg-warning/10 rounded-lg border p-3">
              <p className="mb-2 text-sm font-semibold">Recovery Codes</p>
              <p className="text-fg-secondary mb-2 text-xs">
                Save these codes in a safe place. Each code can only be used once.
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
              aria-label="TOTP code"
              autoComplete="one-time-code"
              className={inputClass + ' max-w-[200px]'}
              inputMode="numeric"
              onChange={(e) => setCode(e.target.value)}
              placeholder="Enter TOTP code"
              value={code}
            />
            <button className={btnPrimary} disabled={submitting} onClick={handleEnable}>
              {submitting ? 'Verifying...' : 'Verify & Enable'}
            </button>
          </div>
        </div>
      )}

      {status?.enabled && (
        <div className={cardClass + ' space-y-3'}>
          <h3 className="text-sm font-medium">Disable 2FA</h3>
          <div className="flex gap-2">
            <input
              aria-label="TOTP code"
              autoComplete="one-time-code"
              className={inputClass + ' max-w-[200px]'}
              inputMode="numeric"
              onChange={(e) => setCode(e.target.value)}
              placeholder="Enter TOTP code"
              value={code}
            />
            <button className={btnDanger} disabled={submitting} onClick={handleDisable}>
              {submitting ? 'Disabling...' : 'Disable 2FA'}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
