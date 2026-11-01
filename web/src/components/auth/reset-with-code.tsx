import { Alert, Button } from '@goliapkg/gds'
import { useState } from 'react'

import { AuthCard } from '@/components/auth/auth-card'
import { AuthField } from '@/components/auth/auth-field'
import { BrandHeader } from '@/components/auth/brand-header'
import { type Key, useT } from '@/i18n'
import { contactText, type ResetContact } from '@/lib/password-clear'
import { wireResetPasswordWithCode } from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

type Props = {
  /** The address the sign-in was tried with, filled in. */
  address: string
  /** Whom to ask for a code; null when the page was opened without a failed sign-in. */
  contact: null | ResetContact
  onCancel: () => void
  /** Called with the address once the new password is set. */
  onDone: (address: string) => void
}

/**
 * Setting a new password on an account whose password an administrator
 * cleared, with the code an issuer handed out. Nothing is signed in: the
 * person signs in afterwards with the new password.
 */
export function ResetWithCode({ address: initialAddress, contact, onCancel, onDone }: Props) {
  const t = useT()
  const [address, setAddress] = useState(initialAddress)
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')
  const [repeat, setRepeat] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (password !== repeat) {
      setError(t('reset.mismatch'))
      return
    }
    setError('')
    setLoading(true)
    try {
      await wireResetPasswordWithCode(address.trim(), code, password)
      onDone(address.trim())
    } catch (err) {
      setError(failureText(err, contactText(contact, t('reset.anAdmin')), t))
    } finally {
      setLoading(false)
    }
  }

  return (
    <AuthCard onSubmit={submit}>
      <BrandHeader subtitle={t('reset.setNew')} />

      <Alert role="status" variant="info">
        {t('reset.byAdmin', { contact: contactText(contact, t('reset.anAdmin')) })}
      </Alert>

      {error && (
        <Alert role="alert" variant="danger">
          {error}
        </Alert>
      )}

      <AuthField
        autoComplete="email"
        id="reset-email"
        label={t('login.email')}
        onChange={setAddress}
        required
        type="email"
        value={address}
      />
      <AuthField
        autoComplete="one-time-code"
        autoFocus
        helperText={t('reset.codeHelp')}
        id="reset-code"
        label={t('reset.code')}
        onChange={setCode}
        required
        value={code}
      />
      <AuthField
        autoComplete="new-password"
        helperText={t('reset.passwordHelp')}
        id="reset-password"
        label={t('reset.newPassword')}
        onChange={setPassword}
        passwordToggle
        required
        type="password"
        value={password}
      />
      <AuthField
        autoComplete="new-password"
        id="reset-repeat"
        label={t('reset.newPasswordAgain')}
        onChange={setRepeat}
        passwordToggle
        required
        type="password"
        value={repeat}
      />

      <Button
        disabled={loading || !address || !code || !password || !repeat}
        fullWidth
        loading={loading}
        type="submit"
        variant="primary"
      >
        {t('reset.setNewButton')}
      </Button>

      <div className="text-center">
        <button className="text-accent text-sm hover:underline" onClick={onCancel} type="button">
          {t('reset.backToSignIn')}
        </button>
      </div>
    </AuthCard>
  )
}

/** What a refused reset says. */
function failureText(
  e: unknown,
  contact: string,
  t: (key: Key, vars?: Record<string, string>) => string
): string {
  if (!(e instanceof WireErrorException)) return t('compose.networkError')
  const d = e.detail
  if (d.kind === 'server' && d.message === 'invalid_reset_code') {
    return t('reset.badCode', { contact })
  }
  if (d.kind === 'server') return d.message
  return e.message
}
