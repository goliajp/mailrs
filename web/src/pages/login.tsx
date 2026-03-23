import { useSetAtom } from 'jotai'
import { Eye, EyeOff, Loader2, ExternalLink } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'

import type { AuthInfo } from '@/store/auth'
import { authAtom } from '@/store/auth'

type OidcClientConfig = { enabled: boolean; login_url?: string; provider_name?: string }

export function Login() {
  const setAuth = useSetAtom(authAtom)
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const [address, setAddress] = useState(() => localStorage.getItem('mailrs_saved_email') ?? '')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [oidcConfig, setOidcConfig] = useState<OidcClientConfig | null>(null)
  const [rememberMe, setRememberMe] = useState(() => !!localStorage.getItem('mailrs_saved_email'))
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
        token,
        address: addr,
        display_name: displayName ?? '',
        permissions: [],
        accessible_domains: [],
      }
      setAuth(auth)
      // refresh permissions by calling /auth/me
      fetch('/api/auth/me', { headers: { Authorization: `Bearer ${token}` } })
        .then((r) => r.json())
        .then((me) => {
          setAuth({
            token,
            address: me.address ?? addr,
            display_name: me.display_name ?? displayName ?? '',
            permissions: me.permissions ?? [],
            accessible_domains: me.accessible_domains ?? [],
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
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ address: forgotAddress, recovery_email: forgotRecoveryEmail }),
      })
      setForgotMessage('If the account and recovery email match, a reset link has been sent.')
    } catch {
      setForgotMessage('If the account and recovery email match, a reset link has been sent.')
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
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          address,
          password,
          ...(totpRequired && totpCode ? { totp_code: totpCode } : {}),
        }),
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
        token: data.token,
        address: data.address,
        display_name: data.display_name,
        permissions: data.permissions ?? [],
        accessible_domains: data.accessible_domains ?? [],
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
        onSubmit={forgotMode ? (e) => e.preventDefault() : handleSubmit}
        className="w-full max-w-sm select-none space-y-5 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-6 shadow-lg sm:p-8"
      >
        <div className="flex flex-col items-center">
          <img src="/icon.svg" alt="mailrs" className="mb-3 h-14 w-14 rounded-lg shadow-sm" />
          <h1 className="text-xl font-semibold tracking-tight text-[var(--color-text-primary)]">
            Mailrs
          </h1>
          <p className="mt-1 text-sm text-[var(--color-text-tertiary)]">
            {forgotMode ? 'Reset your password' : 'Sign in to your account'}
          </p>
        </div>

        {error && !forgotMode && (
          <div
            role="alert"
            className="rounded-md bg-[var(--color-status-danger-subtle)] px-3 py-2 text-sm text-[var(--color-status-danger)]"
          >
            {error}
          </div>
        )}

        {!forgotMode && (
          <>
            <div className="space-y-1.5">
              <label
                htmlFor="login-email"
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
              >
                Email
              </label>
              <input
                id="login-email"
                type="email"
                value={address}
                onChange={(e) => setAddress(e.target.value)}
                placeholder="you@domain.com"
                required
                autoFocus
                aria-label="Email address"
                className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
              />
            </div>

            <div className="space-y-1.5">
              <label
                htmlFor="login-password"
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
              >
                Password
              </label>
              <div className="relative">
                <input
                  id="login-password"
                  type={showPassword ? 'text' : 'password'}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  required
                  aria-label="Password"
                  className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 pr-10 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
                />
                <button
                  type="button"
                  onClick={() => setShowPassword((v) => !v)}
                  aria-label={showPassword ? 'Hide password' : 'Show password'}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
                  tabIndex={-1}
                >
                  {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
            </div>
          </>
        )}

        {!forgotMode && totpRequired && (
          <div className="space-y-1.5">
            <label
              htmlFor="login-totp"
              className="block text-xs font-medium text-[var(--color-text-secondary)]"
            >
              Two-Factor Code
            </label>
            <input
              id="login-totp"
              type="text"
              inputMode="numeric"
              autoComplete="one-time-code"
              value={totpCode}
              onChange={(e) => setTotpCode(e.target.value)}
              placeholder="Enter 6-digit code"
              required
              autoFocus
              aria-label="Two-factor authentication code"
              className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
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
                  type="checkbox"
                  checked={rememberMe}
                  onChange={(e) => setRememberMe(e.target.checked)}
                  className="h-4 w-4 rounded border-[var(--color-border-default)] focus:ring-[var(--color-focus-ring)]"
                />
                <span className="text-sm text-[var(--color-text-secondary)]">Remember email</span>
              </label>
            )}

            <button
              type="submit"
              disabled={loading}
              className="flex w-full items-center justify-center rounded-md bg-[var(--color-brand-primary)] px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-50"
            >
              {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {loading ? 'Signing in...' : totpRequired ? 'Verify' : 'Sign in'}
            </button>

            <div className="text-center">
              <button
                type="button"
                onClick={() => {
                  setForgotMode(true)
                  setError('')
                  setForgotMessage('')
                }}
                className="text-sm text-[var(--color-brand-primary)] hover:underline"
              >
                Forgot password?
              </button>
            </div>

            {oidcConfig?.enabled && (
              <>
                <div className="flex items-center gap-3">
                  <div className="h-px flex-1 bg-[var(--color-border-default)]" />
                  <span className="text-xs text-[var(--color-text-tertiary)]">or</span>
                  <div className="h-px flex-1 bg-[var(--color-border-default)]" />
                </div>
                <a
                  href={oidcConfig.login_url}
                  className="flex w-full items-center justify-center gap-2 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm font-medium text-[var(--color-text-primary)] transition-colors hover:bg-[var(--color-hover)]"
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
                htmlFor="forgot-email"
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
              >
                Account Email
              </label>
              <input
                id="forgot-email"
                type="email"
                value={forgotAddress}
                onChange={(e) => setForgotAddress(e.target.value)}
                placeholder="you@domain.com"
                required
                autoFocus
                className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
              />
            </div>

            <div className="space-y-1.5">
              <label
                htmlFor="forgot-recovery"
                className="block text-xs font-medium text-[var(--color-text-secondary)]"
              >
                Recovery Email
              </label>
              <input
                id="forgot-recovery"
                type="email"
                value={forgotRecoveryEmail}
                onChange={(e) => setForgotRecoveryEmail(e.target.value)}
                placeholder="your-recovery@gmail.com"
                required
                className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:ring-1 focus:ring-[var(--color-focus-ring)]"
              />
              <p className="text-xs text-[var(--color-text-tertiary)]">
                Enter the recovery email you configured in Settings. The reset link will be sent
                there.
              </p>
            </div>

            <button
              type="button"
              onClick={handleForgotPassword}
              disabled={forgotLoading || !forgotAddress || !forgotRecoveryEmail}
              className="flex w-full items-center justify-center rounded-md bg-[var(--color-brand-primary)] px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-50"
            >
              {forgotLoading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {forgotLoading ? 'Sending...' : 'Send Reset Link'}
            </button>

            <div className="text-center">
              <button
                type="button"
                onClick={() => {
                  setForgotMode(false)
                  setForgotMessage('')
                }}
                className="text-sm text-[var(--color-brand-primary)] hover:underline"
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
