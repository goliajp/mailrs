import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useAtomValue, useSetAtom } from 'jotai'
import { useEffect, useState } from 'react'

import { fetchJson, postJson } from '@/lib/api'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import { getToken } from '@/store/auth'
import { authAtom } from '@/store/auth'

import {
  btnDanger,
  btnPrimary,
  cardClass,
  ConfirmDialog,
  Field,
  inputClass,
  SectionHeader,
} from './_shared'

export function AccountSection() {
  const auth = useAtomValue(authAtom)
  const setAuth = useSetAtom(authAtom)
  const [pw, setPw] = useState({ confirm: '', current: '', next: '' })
  const [saving, setSaving] = useState(false)
  const [recoveryEmail, setRecoveryEmail] = useState('')
  const [savingRecovery, setSavingRecovery] = useState(false)
  const [showLogoutConfirm, setShowLogoutConfirm] = useState(false)

  const recoveryQuery = useQuery({
    queryKey: settingsKeys.recoveryEmail(),
    queryFn: async () => {
      try {
        return await fetchJson<{ recovery_email: string }>('/auth/recovery-email')
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
      await postJson('/auth/recovery-email', { recovery_email: recoveryEmail })
      toast.success('Recovery email updated')
      void queryClient.invalidateQueries({ queryKey: settingsKeys.recoveryEmail() })
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update recovery email')
    } finally {
      setSavingRecovery(false)
    }
  }

  const handlePasswordChange = async () => {
    if (!pw.current || !pw.next) return
    if (pw.next !== pw.confirm) {
      toast.error('New passwords do not match')
      return
    }
    if (pw.next.length < 8) {
      toast.error('Password must be at least 8 characters')
      return
    }
    setSaving(true)
    try {
      await postJson('/auth/change-password', {
        current_password: pw.current,
        new_password: pw.next,
      })
      toast.success('Password updated')
      setPw({ confirm: '', current: '', next: '' })
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update password')
    } finally {
      setSaving(false)
    }
  }

  const doLogout = async () => {
    try {
      await postJson('/auth/logout', {})
    } catch {
      // ignore
    }
    setAuth(null)
    window.location.href = '/login'
  }

  return (
    <div className="space-y-6">
      <SectionHeader title="Account" />

      <div className={cardClass}>
        <div className="space-y-3">
          <Field label="Email">
            <span className="text-fg-secondary text-sm">{auth?.address ?? '-'}</span>
          </Field>
          <Field label="Display Name">
            <span className="text-fg-secondary text-sm">{auth?.display_name || '-'}</span>
          </Field>
          <Field label="Permissions">
            <span className="text-fg-secondary text-sm break-all">
              {auth?.permissions?.join(', ') || '-'}
            </span>
          </Field>
          <Field label="Domains">
            <span className="text-fg-secondary text-sm break-all">
              {auth?.accessible_domains?.join(', ') || '-'}
            </span>
          </Field>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-3 text-sm font-medium">Recovery Email</h3>
        <p className="text-fg-muted mb-2 text-xs">
          Used to receive password reset links. Must be an external email address you can access
          independently.
        </p>
        <div className="flex gap-2">
          <input
            className={inputClass + ' flex-1'}
            disabled={!recoveryLoaded}
            onChange={(e) => setRecoveryEmail(e.target.value)}
            placeholder={recoveryLoaded ? 'Not configured' : 'Loading...'}
            type="email"
            value={recoveryEmail}
          />
          <button
            className={btnPrimary}
            disabled={savingRecovery || !recoveryLoaded}
            onClick={handleRecoveryEmailSave}
          >
            {savingRecovery ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-3 text-sm font-medium">Change Password</h3>
        <div className="space-y-2">
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-current-pw"
            >
              Current password
            </label>
            <input
              autoComplete="current-password"
              className={inputClass}
              id="settings-current-pw"
              onChange={(e) => setPw({ ...pw, current: e.target.value })}
              placeholder="Current password"
              type="password"
              value={pw.current}
            />
          </div>
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-new-pw"
            >
              New password
            </label>
            <input
              autoComplete="new-password"
              className={inputClass}
              id="settings-new-pw"
              onChange={(e) => setPw({ ...pw, next: e.target.value })}
              placeholder="New password"
              type="password"
              value={pw.next}
            />
          </div>
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-confirm-pw"
            >
              Confirm new password
            </label>
            <input
              autoComplete="new-password"
              className={inputClass}
              id="settings-confirm-pw"
              onChange={(e) => setPw({ ...pw, confirm: e.target.value })}
              placeholder="Confirm new password"
              type="password"
              value={pw.confirm}
            />
          </div>
          <button className={btnPrimary} disabled={saving} onClick={handlePasswordChange}>
            {saving ? 'Saving...' : 'Update Password'}
          </button>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-2 text-sm font-medium">Export Mailbox</h3>
        <p className="text-fg-muted mb-3 text-xs">Download all your emails as an MBOX file</p>
        <button
          className={btnPrimary}
          onClick={() => window.open(`/api/mail/export?token=${getToken()}`, '_blank')}
        >
          Export as MBOX
        </button>
      </div>

      <div className="border-border border-t pt-4">
        <button className={btnDanger} onClick={() => setShowLogoutConfirm(true)}>
          Sign out
        </button>
        {auth?.address && <p className="text-fg-muted mt-2 text-xs">Signed in as {auth.address}</p>}
      </div>

      {showLogoutConfirm && (
        <ConfirmDialog
          message="Sign out? You will need to sign in again to access your mailbox."
          onCancel={() => setShowLogoutConfirm(false)}
          onConfirm={doLogout}
        />
      )}
    </div>
  )
}
