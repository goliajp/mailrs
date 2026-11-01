import { Alert, Button } from '@goliapkg/gds'
import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'

import { AuthCard } from '@/components/auth/auth-card'
import { AuthField } from '@/components/auth/auth-field'
import { BrandHeader } from '@/components/auth/brand-header'
import { type Key, useT } from '@/i18n'
import { wireResetPassword } from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

export function ResetPassword() {
  const t = useT()
  const [searchParams] = useSearchParams()
  const token = searchParams.get('token') ?? ''
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState('')
  const [success, setSuccess] = useState(false)
  const [loading, setLoading] = useState(false)

  const confirmMismatch = confirm.length > 0 && confirm !== password

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')

    if (password.length < 12) {
      setError(t('reset.tooShort'))
      return
    }

    if (password !== confirm) {
      setError(t('reset.mismatch'))
      return
    }

    setLoading(true)
    try {
      await wireResetPassword(token, password)
      setSuccess(true)
    } catch (e) {
      setError(resetErrorText(e, t))
    } finally {
      setLoading(false)
    }
  }

  if (!token) {
    return (
      <AuthCard>
        <BrandHeader />
        <Alert role="alert" variant="danger">
          {t('reset.badToken')}
        </Alert>
        <div className="text-center">
          <Link className="text-accent text-sm hover:underline" to="/login">
            {t('reset.backToSignIn')}
          </Link>
        </div>
      </AuthCard>
    )
  }

  if (success) {
    return (
      <AuthCard>
        <BrandHeader />
        <Alert role="status" variant="success">
          {t('reset.done')}
        </Alert>
        <div className="text-center">
          <Link className="text-accent text-sm hover:underline" to="/login">
            {t('login.signIn')}
          </Link>
        </div>
      </AuthCard>
    )
  }

  return (
    <AuthCard onSubmit={handleSubmit}>
      <BrandHeader subtitle={t('reset.setYours')} />

      {error && (
        <Alert role="alert" variant="danger">
          {error}
        </Alert>
      )}

      <AuthField
        autoComplete="new-password"
        autoFocus
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
        id="reset-confirm"
        invalid={confirmMismatch}
        invalidMessage={mismatchMessage(confirmMismatch, t('reset.mismatch'))}
        label={t('reset.confirm')}
        onChange={setConfirm}
        passwordToggle
        required
        type="password"
        value={confirm}
      />

      <Button disabled={loading} fullWidth loading={loading} type="submit" variant="primary">
        {t(resetLabel(loading))}
      </Button>

      <div className="text-center">
        <Link className="text-accent text-sm hover:underline" to="/login">
          {t('reset.backToSignIn')}
        </Link>
      </div>
    </AuthCard>
  )
}

function mismatchMessage(mismatch: boolean, text: string): string | undefined {
  if (mismatch) return text
  return undefined
}

function resetErrorText(e: unknown, t: (key: Key) => string): string {
  if (!(e instanceof WireErrorException)) return t('compose.networkError')
  if (e.detail.kind === 'server') return e.detail.message ?? t('reset.failed')
  if (e.detail.kind === 'network') return t('compose.networkError')
  return t('reset.failed')
}

function resetLabel(loading: boolean): Key {
  if (loading) return 'reset.resetting'
  return 'reset.submit'
}
