import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { useSearchParams } from 'react-router'

export function ResetPassword() {
  const [searchParams] = useSearchParams()
  const token = searchParams.get('token') ?? ''
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState('')
  const [success, setSuccess] = useState(false)
  const [loading, setLoading] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')

    if (!token) {
      setError('Missing reset token')
      return
    }

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
      const res = await fetch('/api/auth/reset-password', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ token, new_password: password }),
      })
      const data = await res.json()

      if (!res.ok) {
        setError(data.error ?? 'Failed to reset password')
        return
      }

      setSuccess(true)
    } catch {
      setError('Network error')
    } finally {
      setLoading(false)
    }
  }

  if (!token) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-[var(--color-bg-sunken)]">
        <div className="w-full max-w-sm space-y-4 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-8" style={{ boxShadow: 'var(--shadow-lg)' }}>
          <div className="flex flex-col items-center">
            <img src="/icon.svg" alt="mailrs" className="mb-3 h-14 w-14 rounded-lg" style={{ boxShadow: 'var(--shadow-sm)' }} />
            <h1 className="text-xl font-semibold tracking-tight text-[var(--color-text-primary)]">mailrs</h1>
          </div>
          <div className="rounded-md bg-[var(--color-status-danger-subtle)] px-3 py-2 text-sm text-[var(--color-status-danger)]" role="alert">
            Invalid or missing reset token
          </div>
          <div className="text-center">
            <a href="/login" className="text-sm text-[var(--color-brand-primary)] hover:underline">
              Back to sign in
            </a>
          </div>
        </div>
      </div>
    )
  }

  if (success) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-[var(--color-bg-sunken)]">
        <div className="w-full max-w-sm space-y-4 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-8" style={{ boxShadow: 'var(--shadow-lg)' }}>
          <div className="flex flex-col items-center">
            <img src="/icon.svg" alt="mailrs" className="mb-3 h-14 w-14 rounded-lg" style={{ boxShadow: 'var(--shadow-sm)' }} />
            <h1 className="text-xl font-semibold tracking-tight text-[var(--color-text-primary)]">mailrs</h1>
          </div>
          <div className="rounded-md bg-[var(--color-status-success-subtle)] px-3 py-2 text-sm text-[var(--color-status-success)]">
            Password reset successfully. You can now sign in with your new password.
          </div>
          <div className="text-center">
            <a href="/login" className="text-sm text-[var(--color-brand-primary)] hover:underline">
              Sign in
            </a>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-[var(--color-bg-sunken)]">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-sm select-none space-y-5 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-8"
        style={{ boxShadow: 'var(--shadow-lg)' }}
      >
        <div className="flex flex-col items-center">
          <img src="/icon.svg" alt="mailrs" className="mb-3 h-14 w-14 rounded-lg" style={{ boxShadow: 'var(--shadow-sm)' }} />
          <h1 className="text-xl font-semibold tracking-tight text-[var(--color-text-primary)]">
            mailrs
          </h1>
          <p className="mt-1 text-sm text-[var(--color-text-tertiary)]">
            Set your new password
          </p>
        </div>

        {error && (
          <div
            role="alert"
            className="rounded-md bg-[var(--color-status-danger-subtle)] px-3 py-2 text-sm text-[var(--color-status-danger)]"
          >
            {error}
          </div>
        )}

        <div className="space-y-1.5">
          <label
            htmlFor="reset-password"
            className="block text-sm font-medium text-[var(--color-text-secondary)]"
          >
            New Password
          </label>
          <input
            id="reset-password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            autoFocus
            aria-label="New password"
            className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
          />
        </div>

        <div className="space-y-1.5">
          <label
            htmlFor="reset-confirm"
            className="block text-sm font-medium text-[var(--color-text-secondary)]"
          >
            Confirm Password
          </label>
          <input
            id="reset-confirm"
            type="password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            required
            aria-label="Confirm password"
            className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
          />
        </div>

        <button
          type="submit"
          disabled={loading}
          className="flex w-full items-center justify-center rounded-md bg-[var(--color-brand-primary)] px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-50"
        >
          {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {loading ? 'Resetting...' : 'Reset Password'}
        </button>

        <div className="text-center">
          <a href="/login" className="text-sm text-[var(--color-brand-primary)] hover:underline">
            Back to sign in
          </a>
        </div>
      </form>
    </div>
  )
}
