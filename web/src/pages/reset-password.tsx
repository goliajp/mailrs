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
        body: JSON.stringify({ new_password: password, token }),
        headers: { 'Content-Type': 'application/json' },
        method: 'POST',
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
      <div className="bg-bg-secondary flex min-h-screen items-center justify-center">
        <div className="border-border bg-surface w-full max-w-sm space-y-4 rounded-lg border p-8 shadow-lg">
          <div className="flex flex-col items-center">
            <img alt="mailrs" className="mb-3 h-14 w-14 rounded-lg shadow-sm" src="/icon.svg" />
            <h1 className="text-fg text-xl font-semibold tracking-tight">mailrs</h1>
          </div>
          <div className="bg-danger/10 text-danger rounded-md px-3 py-2 text-sm" role="alert">
            Invalid or missing reset token
          </div>
          <div className="text-center">
            <a className="text-accent text-sm hover:underline" href="/login">
              Back to sign in
            </a>
          </div>
        </div>
      </div>
    )
  }

  if (success) {
    return (
      <div className="bg-bg-secondary flex min-h-screen items-center justify-center">
        <div className="border-border bg-surface w-full max-w-sm space-y-4 rounded-lg border p-8 shadow-lg">
          <div className="flex flex-col items-center">
            <img alt="mailrs" className="mb-3 h-14 w-14 rounded-lg shadow-sm" src="/icon.svg" />
            <h1 className="text-fg text-xl font-semibold tracking-tight">mailrs</h1>
          </div>
          <div className="bg-success/10 text-success rounded-md px-3 py-2 text-sm">
            Password reset successfully. You can now sign in with your new password.
          </div>
          <div className="text-center">
            <a className="text-accent text-sm hover:underline" href="/login">
              Sign in
            </a>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="bg-bg-secondary flex min-h-screen items-center justify-center">
      <form
        className="border-border bg-surface w-full max-w-sm space-y-5 rounded-lg border p-8 shadow-lg select-none"
        onSubmit={handleSubmit}
      >
        <div className="flex flex-col items-center">
          <img alt="mailrs" className="mb-3 h-14 w-14 rounded-lg shadow-sm" src="/icon.svg" />
          <h1 className="text-fg text-xl font-semibold tracking-tight">mailrs</h1>
          <p className="text-fg-muted mt-1 text-sm">Set your new password</p>
        </div>

        {error && (
          <div className="bg-danger/10 text-danger rounded-md px-3 py-2 text-sm" role="alert">
            {error}
          </div>
        )}

        <div className="space-y-1.5">
          <label className="text-fg-secondary block text-sm font-medium" htmlFor="reset-password">
            New Password
          </label>
          <input
            aria-label="New password"
            autoFocus
            className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 text-sm outline-none focus:ring-1"
            id="reset-password"
            onChange={(e) => setPassword(e.target.value)}
            required
            type="password"
            value={password}
          />
        </div>

        <div className="space-y-1.5">
          <label className="text-fg-secondary block text-sm font-medium" htmlFor="reset-confirm">
            Confirm Password
          </label>
          <input
            aria-label="Confirm password"
            className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 text-sm outline-none focus:ring-1"
            id="reset-confirm"
            onChange={(e) => setConfirm(e.target.value)}
            required
            type="password"
            value={confirm}
          />
        </div>

        <button
          className="bg-accent hover:bg-accent-hover flex w-full items-center justify-center rounded-md px-3 py-2 text-sm font-medium text-white transition-colors disabled:opacity-50"
          disabled={loading}
          type="submit"
        >
          {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {loading ? 'Resetting...' : 'Reset Password'}
        </button>

        <div className="text-center">
          <a className="text-accent text-sm hover:underline" href="/login">
            Back to sign in
          </a>
        </div>
      </form>
    </div>
  )
}
