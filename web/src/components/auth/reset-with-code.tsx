import { Alert, Button } from '@goliapkg/gds'
import { useState } from 'react'

import { AuthCard } from '@/components/auth/auth-card'
import { AuthField } from '@/components/auth/auth-field'
import { BrandHeader } from '@/components/auth/brand-header'
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
  const [address, setAddress] = useState(initialAddress)
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')
  const [repeat, setRepeat] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (password !== repeat) {
      setError('The two passwords are not the same')
      return
    }
    setError('')
    setLoading(true)
    try {
      await wireResetPasswordWithCode(address.trim(), code, password)
      onDone(address.trim())
    } catch (err) {
      setError(failureText(err, contact))
    } finally {
      setLoading(false)
    }
  }

  return (
    <AuthCard onSubmit={submit}>
      <BrandHeader subtitle="Set a new password" />

      <Alert role="status" variant="info">
        Your password was reset by an administrator. Ask {contactText(contact)} for a reset code,
        then set a new password here.
      </Alert>

      {error && (
        <Alert role="alert" variant="danger">
          {error}
        </Alert>
      )}

      <AuthField
        autoComplete="email"
        id="reset-email"
        label="Email"
        onChange={setAddress}
        required
        type="email"
        value={address}
      />
      <AuthField
        autoComplete="one-time-code"
        autoFocus
        helperText="10 characters, given to you in person"
        id="reset-code"
        label="Reset code"
        onChange={setCode}
        required
        value={code}
      />
      <AuthField
        autoComplete="new-password"
        helperText="At least 12 characters. Leave out your name and the company name"
        id="reset-password"
        label="New password"
        onChange={setPassword}
        passwordToggle
        required
        type="password"
        value={password}
      />
      <AuthField
        autoComplete="new-password"
        id="reset-repeat"
        label="New password again"
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
        Set new password
      </Button>

      <div className="text-center">
        <button className="text-accent text-sm hover:underline" onClick={onCancel} type="button">
          Back to sign in
        </button>
      </div>
    </AuthCard>
  )
}

/** What a refused reset says. */
function failureText(e: unknown, contact: null | ResetContact): string {
  if (!(e instanceof WireErrorException)) return 'Network error'
  const d = e.detail
  if (d.kind === 'server' && d.message === 'invalid_reset_code') {
    return `That code does not work: it may be mistyped, used up, or replaced by a newer one. Ask ${contactText(contact)} for a new code.`
  }
  if (d.kind === 'server') return d.message
  return e.message
}
