import type { AuthInfo } from '@/store/auth'

import { Alert, Button } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { ExternalLink } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'

import { AuthCard } from '@/components/auth/auth-card'
import { AuthField } from '@/components/auth/auth-field'
import { BrandHeader } from '@/components/auth/brand-header'
import { authAtom } from '@/store/auth'
import { wireForgotPassword, wireGetMe, wireGetOidcConfig, wireLogin } from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

type OidcClientConfig = {
  enabled: boolean
  login_url?: string
  provider_name?: string
}

export function Login() {
  const setAuth = useSetAtom(authAtom)
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const [address, setAddress] = useState(() => localStorage.getItem('mailrs_saved_email') ?? '')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [oidcConfig, setOidcConfig] = useState<null | OidcClientConfig>(null)
  const [rememberMe, setRememberMe] = useState(() => !!localStorage.getItem('mailrs_saved_email'))
  const [totpRequired, setTotpRequired] = useState(false)
  const [totpCode, setTotpCode] = useState('')
  const [forgotMode, setForgotMode] = useState(false)
  const [forgotAddress, setForgotAddress] = useState('')
  const [forgotRecoveryEmail, setForgotRecoveryEmail] = useState('')
  const [forgotLoading, setForgotLoading] = useState(false)
  const [forgotMessage, setForgotMessage] = useState('')

  // fetch OIDC config
  useEffect(() => {
    wireGetOidcConfig()
      .then((cfg) => setOidcConfig(cfg as unknown as OidcClientConfig))
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
      wireGetMe(token)
        .then((me) => {
          setAuth({
            accessible_domains: me.accessible_domains,
            address: me.address || addr,
            display_name: me.display_name || (displayName ?? ''),
            permissions: me.permissions,
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
      await wireForgotPassword(forgotAddress, forgotRecoveryEmail || undefined)
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
      const data = await wireLogin(
        address,
        password,
        totpRequired && totpCode ? totpCode : undefined
      )

      // server asks for TOTP code
      if (data.requires_totp === true) {
        setTotpRequired(true)
        return
      }

      const auth: AuthInfo = {
        accessible_domains: data.accessible_domains,
        address: data.address,
        display_name: data.display_name,
        permissions: data.permissions,
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
    } catch (e) {
      if (e instanceof WireErrorException) {
        setError(
          e.detail.kind === 'server'
            ? (e.detail.message ?? 'Login failed')
            : e.detail.kind === 'network'
              ? 'Network error'
              : e.detail.kind === 'auth'
                ? 'Login failed'
                : 'Login failed'
        )
      } else {
        setError('Network error')
      }
    } finally {
      setLoading(false)
    }
  }

  if (forgotMode) {
    return (
      <AuthCard onSubmit={handleForgotPassword}>
        <BrandHeader subtitle="Reset your password" />

        {forgotMessage && (
          <Alert role="status" variant="success">
            {forgotMessage}
          </Alert>
        )}

        <AuthField
          autoComplete="email"
          autoFocus
          id="forgot-email"
          label="Account Email"
          onChange={setForgotAddress}
          placeholder="you@domain.com"
          required
          type="email"
          value={forgotAddress}
        />

        <AuthField
          autoComplete="email"
          helperText="Enter the recovery email you configured in Settings. The reset link will be sent there."
          id="forgot-recovery"
          label="Recovery Email"
          onChange={setForgotRecoveryEmail}
          placeholder="your-recovery@gmail.com"
          required
          type="email"
          value={forgotRecoveryEmail}
        />

        <Button
          disabled={forgotLoading || !forgotAddress || !forgotRecoveryEmail}
          fullWidth
          loading={forgotLoading}
          type="submit"
          variant="primary"
        >
          {forgotLoading ? 'Sending...' : 'Send Reset Link'}
        </Button>

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
      </AuthCard>
    )
  }

  return (
    <AuthCard onSubmit={handleSubmit}>
      <BrandHeader subtitle="Sign in to your account" />

      {error && (
        <Alert role="alert" variant="danger">
          {error}
        </Alert>
      )}

      {!totpRequired && (
        <>
          <AuthField
            autoComplete="email"
            autoFocus
            id="login-email"
            label="Email"
            onChange={setAddress}
            placeholder="you@domain.com"
            required
            type="email"
            value={address}
          />

          <AuthField
            autoComplete="current-password"
            id="login-password"
            label="Password"
            onChange={setPassword}
            passwordToggle
            required
            type="password"
            value={password}
          />
        </>
      )}

      {totpRequired && (
        <AuthField
          autoComplete="one-time-code"
          autoFocus
          helperText="Enter the code from your authenticator app, or a recovery code"
          id="login-totp"
          inputMode="numeric"
          label="Two-Factor Code"
          onChange={setTotpCode}
          placeholder="Enter 6-digit code"
          required
          value={totpCode}
        />
      )}

      {!totpRequired && (
        <label className="flex items-center gap-2">
          <input
            checked={rememberMe}
            className="border-border focus:ring-accent/40 h-4 w-4 rounded"
            onChange={(e) => setRememberMe(e.target.checked)}
            type="checkbox"
          />
          <span className="text-fg-secondary text-sm">Remember email</span>
        </label>
      )}

      <Button disabled={loading} fullWidth loading={loading} type="submit" variant="primary">
        {loading ? 'Signing in...' : totpRequired ? 'Verify' : 'Sign in'}
      </Button>

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
    </AuthCard>
  )
}
