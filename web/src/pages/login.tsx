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

      // follow return_to if present (OIDC authorize flow)
      const returnTo = searchParams.get('return_to')
      if (returnTo) {
        window.location.href = returnTo
      } else {
        navigate('/', { replace: true })
      }
    } catch {
      setError('Network error')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="bg-bg flex min-h-screen items-center justify-center px-4">
      <form
        className="border-border bg-surface w-full max-w-sm space-y-5 rounded-lg border p-6 shadow-lg select-none sm:p-8"
        onSubmit={forgotMode ? (e) => e.preventDefault() : handleSubmit}
      >
        <div className="flex flex-col items-center">
          <img
            alt="mailrs"
            className="mb-3 h-14 w-14 rounded-lg shadow-sm"
            src="/icon.svg"
          />
          <h1 className="text-fg text-xl font-semibold tracking-tight">
            Mailrs
          </h1>
          <p className="text-fg-muted mt-1 text-sm">
            {forgotMode ? 'Reset your password' : 'Sign in to your account'}
          </p>
        </div>

        {error && !forgotMode && (
          <div
            className="bg-danger/10 text-danger rounded-md px-3 py-2 text-sm"
            role="alert"
          >
            {error}
          </div>
        )}

        {!forgotMode && (
          <>
            <div className="space-y-1.5">
              <label
                className="text-fg-secondary block text-xs font-medium"
                htmlFor="login-email"
              >
                Email
              </label>
              <input
                aria-label="Email address"
                autoFocus
                className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 text-sm outline-none focus:ring-1"
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
                className="text-fg-secondary block text-xs font-medium"
                htmlFor="login-password"
              >
                Password
              </label>
              <div className="relative">
                <input
                  aria-label="Password"
                  className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 pr-10 text-sm outline-none focus:ring-1"
                  id="login-password"
                  onChange={(e) => setPassword(e.target.value)}
                  required
                  type={showPassword ? 'text' : 'password'}
                  value={password}
                />
                <button
                  aria-label={showPassword ? 'Hide password' : 'Show password'}
                  className="text-fg-muted hover:text-fg-secondary absolute top-1/2 right-2 -translate-y-1/2"
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
              className="text-fg-secondary block text-xs font-medium"
              htmlFor="login-totp"
            >
              Two-Factor Code
            </label>
            <input
              aria-label="Two-factor authentication code"
              autoComplete="one-time-code"
              autoFocus
              className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 text-sm outline-none focus:ring-1"
              id="login-totp"
              inputMode="numeric"
              onChange={(e) => setTotpCode(e.target.value)}
              placeholder="Enter 6-digit code"
              required
              type="text"
              value={totpCode}
            />
            <p className="text-fg-muted text-xs">
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
                  className="border-border focus:ring-accent/40 h-4 w-4 rounded"
                  onChange={(e) => setRememberMe(e.target.checked)}
                  type="checkbox"
                />
                <span className="text-fg-secondary text-sm">
                  Remember email
                </span>
              </label>
            )}

            <button
              className="bg-accent hover:bg-accent-hover flex w-full items-center justify-center rounded-md px-3 py-2 text-sm font-medium text-white transition-colors disabled:opacity-50"
              disabled={loading}
              type="submit"
            >
              {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {loading ? 'Signing in...' : totpRequired ? 'Verify' : 'Sign in'}
            </button>

            <div className="text-center">
              <button
                className="text-accent text-sm hover:underline"
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
                  <div className="bg-border h-px flex-1" />
                  <span className="text-fg-muted text-xs">or</span>
                  <div className="bg-border h-px flex-1" />
                </div>
                <a
                  className="border-border bg-bg-secondary text-fg hover:bg-bg-secondary flex w-full items-center justify-center gap-2 rounded-md border px-3 py-2 text-sm font-medium transition-colors"
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
              <div className="bg-success/10 text-success rounded-md px-3 py-2 text-sm">
                {forgotMessage}
              </div>
            )}

            <div className="space-y-1.5">
              <label
                className="text-fg-secondary block text-xs font-medium"
                htmlFor="forgot-email"
              >
                Account Email
              </label>
              <input
                autoFocus
                className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 text-sm outline-none focus:ring-1"
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
                className="text-fg-secondary block text-xs font-medium"
                htmlFor="forgot-recovery"
              >
                Recovery Email
              </label>
              <input
                className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 text-sm outline-none focus:ring-1"
                id="forgot-recovery"
                onChange={(e) => setForgotRecoveryEmail(e.target.value)}
                placeholder="your-recovery@gmail.com"
                required
                type="email"
                value={forgotRecoveryEmail}
              />
              <p className="text-fg-muted text-xs">
                Enter the recovery email you configured in Settings. The reset
                link will be sent there.
              </p>
            </div>

            <button
              className="bg-accent hover:bg-accent-hover flex w-full items-center justify-center rounded-md px-3 py-2 text-sm font-medium text-white transition-colors disabled:opacity-50"
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
                className="text-accent text-sm hover:underline"
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
