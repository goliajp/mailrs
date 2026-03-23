import type { AuthInfo } from '@/store/auth'

import { useSetAtom } from 'jotai'
import { ExternalLink, Eye, EyeOff, Loader2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'

import { authAtom } from '@/store/auth'

type OidcClientConfig = {
  enabled: boolean
  login_url?: string
  provider_name?: string
}

export function Login() {
  const setAuth = useSetAtom(authAtom)
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const [address, setAddress] = useState(
    () => localStorage.getItem('mailrs_saved_email') ?? ''
  )
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [oidcConfig, setOidcConfig] = useState<null | OidcClientConfig>(null)
  const [rememberMe, setRememberMe] = useState(
    () => !!localStorage.getItem('mailrs_saved_email')
  )
  const [showPassword, setShowPassword] = useState(false)
  const [totpRequired, setTotpRequired] = useState(false)
  const [totpCode, setTotpCode] = useState('')
  const [forgotMode, setForgotMode] = useState(false)
  const [forgotAddress, setForgotAddress] = useState('')
  const [forgotRecoveryEmail, setForgotRecoveryEmail] = useState('')
  const [forgotLoading, setForgotLoading] = useState(false)
  const [forgotMessage, setForgotMessage] = useState('')

  // fetch OIDC config
  useEffect(() => {
    fetch('/api/auth/oidc/config')
      .then((r) => r.json())
      .then(setOidcConfig)
      .catch(() => {})
  }, [])

  // handle OIDC callback redirect
  useEffect(() => {
    const token = searchParams.get('oidc_token')
    const addr = searchParams.get('address')
    const displayName = searchParams.get('display_name')
    if (token && addr) {
      const auth: AuthInfo = {
        accessible_domains: [],
        address: addr,
        display_name: displayName ?? '',
        permissions: [],
        token,
      }
      setAuth(auth)
      // refresh permissions by calling /auth/me
      fetch('/api/auth/me', { headers: { Authorization: `Bearer ${token}` } })
        .then((r) => r.json())
        .then((me) => {
          setAuth({
            accessible_domains: me.accessible_domains ?? [],
            address: me.address ?? addr,
            display_name: me.display_name ?? displayName ?? '',
            permissions: me.permissions ?? [],
            token,
          })
        })
        .catch(() => {})
      navigate('/', { replace: true })
    }
  }, [searchParams, setAuth, navigate])

  const handleForgotPassword = async (e: React.FormEvent) => {
    e.preventDefault()
    setForgotLoading(true)
    setForgotMessage('')
    try {
      await fetch('/api/auth/forgot-password', {
        body: JSON.stringify({
          address: forgotAddress,
          recovery_email: forgotRecoveryEmail,
        }),
        headers: { 'Content-Type': 'application/json' },
        method: 'POST',
      })
      setForgotMessage(
        'If the account and recovery email match, a reset link has been sent.'
      )
    } catch {
      setForgotMessage(
        'If the account and recovery email match, a reset link has been sent.'
      )
    } finally {
      setForgotLoading(false)
    }
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)

    try {
      const res = await fetch('/api/auth/login', {
        body: JSON.stringify({
          address,
          password,
          ...(totpRequired && totpCode ? { totp_code: totpCode } : {}),
        }),
        headers: { 'Content-Type': 'application/json' },
        method: 'POST',
      })

      const data = await res.json()

      if (!res.ok) {
        setError(data.error ?? 'Login failed')
        return
      }

      // server asks for TOTP code
      if (data.requires_totp) {
        setTotpRequired(true)
        return
      }

      const auth: AuthInfo = {
        accessible_domains: data.accessible_domains ?? [],
        address: data.address,
        display_name: data.display_name,
        permissions: data.permissions ?? [],
        token: data.token,
      }
      if (rememberMe) {
        localStorage.setItem('mailrs_saved_email', address)
      } else {
        localStorage.removeItem('mailrs_saved_email')
      }
      setAuth(auth)
      navigate('/', { replace: true })
    } catch {
      setError('Network error')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-[var(--color-bg-base)] px-4">
      <form
        className="w-full max-w-sm space-y-5 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-6 shadow-lg select-none sm:p-8"
        onSubmit={forgotMode ? (e) => e.preventDefault() : handleSubmit}
      >
        <div className="flex flex-col items-center">
          <img
            alt="mailrs"
            className="mb-3 h-14 w-14 rounded-lg shadow-sm"
            src="/icon.svg"
          />
          <h1 className="text-xl font-semibold tracking-tight text-[var(--color-text-primary)]">
            Mailrs
          </h1>
          <p className="mt-1 text-sm text-[var(--color-text-tertiary)]">
            {forgotMode ? 'Reset your password' : 'Sign in to your account'}
          </p>
        </div>

        {error && !forgotMode && (
          <div
            className="rounded-md bg-[var(--color-status-danger-subtle)] px-3 py-2 text-sm text-[var(--color-status-danger)]"
            role="alert"
          >
            {error}
          </div>
        )}

        {!forgotMode && (
          <>
            <div className="space-y-1.5">
              <label
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
                htmlFor="login-email"
              >
                Email
              </label>
              <input
                aria-label="Email address"
                autoFocus
                className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
                id="login-email"
                onChange={(e) => setAddress(e.target.value)}
                placeholder="you@domain.com"
                required
                type="email"
                value={address}
              />
            </div>

            <div className="space-y-1.5">
              <label
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
                htmlFor="login-password"
              >
                Password
              </label>
              <div className="relative">
                <input
                  aria-label="Password"
                  className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 pr-10 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
                  id="login-password"
                  onChange={(e) => setPassword(e.target.value)}
                  required
                  type={showPassword ? 'text' : 'password'}
                  value={password}
                />
                <button
                  aria-label={showPassword ? 'Hide password' : 'Show password'}
                  className="absolute top-1/2 right-2 -translate-y-1/2 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
                  onClick={() => setShowPassword((v) => !v)}
                  tabIndex={-1}
                  type="button"
                >
                  {showPassword ? (
                    <EyeOff className="h-4 w-4" />
                  ) : (
                    <Eye className="h-4 w-4" />
                  )}
                </button>
              </div>
            </div>
          </>
        )}

        {!forgotMode && totpRequired && (
          <div className="space-y-1.5">
            <label
              className="block text-xs font-medium text-[var(--color-text-secondary)]"
              htmlFor="login-totp"
            >
              Two-Factor Code
            </label>
            <input
              aria-label="Two-factor authentication code"
              autoComplete="one-time-code"
              autoFocus
              className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
              id="login-totp"
              inputMode="numeric"
              onChange={(e) => setTotpCode(e.target.value)}
              placeholder="Enter 6-digit code"
              required
              type="text"
              value={totpCode}
            />
            <p className="text-xs text-[var(--color-text-tertiary)]">
              Enter the code from your authenticator app, or a recovery code
            </p>
          </div>
        )}

        {!forgotMode && (
          <>
            {!totpRequired && (
              <label className="flex items-center gap-2">
                <input
                  checked={rememberMe}
                  className="h-4 w-4 rounded border-[var(--color-border-default)] focus:ring-[var(--color-focus-ring)]"
                  onChange={(e) => setRememberMe(e.target.checked)}
                  type="checkbox"
                />
                <span className="text-sm text-[var(--color-text-secondary)]">
                  Remember email
                </span>
              </label>
            )}

            <button
              className="flex w-full items-center justify-center rounded-md bg-[var(--color-brand-primary)] px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-50"
              disabled={loading}
              type="submit"
            >
              {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {loading ? 'Signing in...' : totpRequired ? 'Verify' : 'Sign in'}
            </button>

            <div className="text-center">
              <button
                className="text-sm text-[var(--color-brand-primary)] hover:underline"
                onClick={() => {
                  setForgotMode(true)
                  setError('')
                  setForgotMessage('')
                }}
                type="button"
              >
                Forgot password?
              </button>
            </div>

            {oidcConfig?.enabled && (
              <>
                <div className="flex items-center gap-3">
                  <div className="h-px flex-1 bg-[var(--color-border-default)]" />
                  <span className="text-xs text-[var(--color-text-tertiary)]">
                    or
                  </span>
                  <div className="h-px flex-1 bg-[var(--color-border-default)]" />
                </div>
                <a
                  className="flex w-full items-center justify-center gap-2 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm font-medium text-[var(--color-text-primary)] transition-colors hover:bg-[var(--color-hover)]"
                  href={oidcConfig.login_url}
                >
                  <ExternalLink className="h-4 w-4" />
                  Sign in with {oidcConfig.provider_name ?? 'SSO'}
                </a>
              </>
            )}
          </>
        )}

        {forgotMode && (
          <>
            {forgotMessage && (
              <div className="rounded-md bg-[var(--color-status-success-subtle)] px-3 py-2 text-sm text-[var(--color-status-success)]">
                {forgotMessage}
              </div>
            )}

            <div className="space-y-1.5">
              <label
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
                htmlFor="forgot-email"
              >
                Account Email
              </label>
              <input
                autoFocus
                className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
                id="forgot-email"
                onChange={(e) => setForgotAddress(e.target.value)}
                placeholder="you@domain.com"
                required
                type="email"
                value={forgotAddress}
              />
            </div>

            <div className="space-y-1.5">
              <label
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
                htmlFor="forgot-recovery"
              >
                Recovery Email
              </label>
              <input
                className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
                id="forgot-recovery"
                onChange={(e) => setForgotRecoveryEmail(e.target.value)}
                placeholder="your-recovery@gmail.com"
                required
                type="email"
                value={forgotRecoveryEmail}
              />
              <p className="text-xs text-[var(--color-text-tertiary)]">
                Enter the recovery email you configured in Settings. The reset
                link will be sent there.
              </p>
            </div>

            <button
              className="flex w-full items-center justify-center rounded-md bg-[var(--color-brand-primary)] px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-50"
              disabled={forgotLoading || !forgotAddress || !forgotRecoveryEmail}
              onClick={handleForgotPassword}
              type="button"
            >
              {forgotLoading && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {forgotLoading ? 'Sending...' : 'Send Reset Link'}
            </button>

            <div className="text-center">
              <button
                className="text-sm text-[var(--color-brand-primary)] hover:underline"
                onClick={() => {
                  setForgotMode(false)
                  setForgotMessage('')
                }}
                type="button"
              >
                Back to login
              </button>
            </div>
          </>
        )}
      </form>
    </div>
  )
}
