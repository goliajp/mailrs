import { Alert, Button } from '@goliapkg/gds'
import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'

import { AuthCard } from '@/components/auth/auth-card'
import { AuthField } from '@/components/auth/auth-field'
import { BrandHeader } from '@/components/auth/brand-header'
import { wireResetPassword } from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

export function ResetPassword() {
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

    if (password.length < 8) {
      setError('Password must be at least 8 characters')
      return
    }

    if (password !== confirm) {
      setError('Passwords do not match')
      return
    }

    setLoading(true)
    try {
      await wireResetPassword(token, password)
      setSuccess(true)
    } catch (e) {
      if (e instanceof WireErrorException) {
        setError(
          e.detail.kind === 'server'
            ? (e.detail.message ?? 'Failed to reset password')
            : e.detail.kind === 'network'
              ? 'Network error'
              : 'Failed to reset password'
        )
      } else {
        setError('Network error')
      }
    } finally {
      setLoading(false)
    }
  }

  if (!token) {
    return (
      <AuthCard>
        <BrandHeader />
        <Alert role="alert" variant="danger">
          Invalid or missing reset token
        </Alert>
        <div className="text-center">
          <Link className="text-accent text-sm hover:underline" to="/login">
            Back to sign in
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
          Password reset successfully. You can now sign in with your new password.
        </Alert>
        <div className="text-center">
          <Link className="text-accent text-sm hover:underline" to="/login">
            Sign in
          </Link>
        </div>
      </AuthCard>
    )
  }

  return (
    <AuthCard onSubmit={handleSubmit}>
      <BrandHeader subtitle="Set your new password" />

      {error && (
        <Alert role="alert" variant="danger">
          {error}
        </Alert>
      )}

      <AuthField
        autoComplete="new-password"
        autoFocus
        id="reset-password"
        label="New Password"
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
        invalidMessage={confirmMismatch ? 'Passwords do not match' : undefined}
        label="Confirm Password"
        onChange={setConfirm}
        passwordToggle
        required
        type="password"
        value={confirm}
      />

      <Button disabled={loading} fullWidth loading={loading} type="submit" variant="primary">
        {loading ? 'Resetting...' : 'Reset Password'}
      </Button>

      <div className="text-center">
        <Link className="text-accent text-sm hover:underline" to="/login">
          Back to sign in
        </Link>
      </div>
    </AuthCard>
  )
}
