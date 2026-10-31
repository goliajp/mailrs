import type { AuthInfo } from '@/store/auth'

import { Alert, Button } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { ExternalLink } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'

import { AuthCard } from '@/components/auth/auth-card'
import { AuthField } from '@/components/auth/auth-field'
import { BrandHeader } from '@/components/auth/brand-header'
import { ResetWithCode } from '@/components/auth/reset-with-code'
import { type Key, useT } from '@/i18n'
import { type ResetContact } from '@/lib/password-clear'
import { authAtom } from '@/store/auth'
import {
  wireExternalProviders,
  wireForgotPassword,
  wireGetMe,
  wireGetOidcConfig,
  wireLogin,
} from '@/wire/endpoints/auth'
import { WireErrorException } from '@/wire/errors'

type OidcClientConfig = {
  enabled: boolean
  login_url?: string
  provider_name?: string
}

/** Display names for the provider keys the backend reports. */
const PROVIDER_LABELS: Record<string, string> = {
  apple: 'Apple',
  github: 'GitHub',
  google: 'Google',
  microsoft: 'Microsoft',
  portal: 'GOLIA Portal',
}

/**
 * What to say when the browser comes back from a provider.
 *
 * `link` is not an error and must not read like one: the identity
 * authenticated fine, it just does not belong to a mailbox yet, and the
 * password below is what attaches it. Every other value is a genuine
 * failure and says which.
 */
const EXTERNAL_MESSAGES: Record<string, { text: Key; tone: 'error' | 'info' }> = {
  denied: { text: 'login.external.denied', tone: 'info' },
  expired: { text: 'login.external.expired', tone: 'error' },
  failed: { text: 'login.external.failed', tone: 'error' },
  incomplete: { text: 'login.external.incomplete', tone: 'error' },
  link: { text: 'login.external.link', tone: 'info' },
  'no-account': { text: 'login.external.noAccount', tone: 'error' },
  unavailable: { text: 'login.external.unavailable', tone: 'error' },
}

export function Login() {
  const t = useT()
  const setAuth = useSetAtom(authAtom)
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  const [address, setAddress] = useState(() => localStorage.getItem('mailrs_saved_email') ?? '')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [oidcConfig, setOidcConfig] = useState<null | OidcClientConfig>(null)
  const [externalProviders, setExternalProviders] = useState<string[]>([])
  const [rememberMe, setRememberMe] = useState(() => !!localStorage.getItem('mailrs_saved_email'))
  const [totpRequired, setTotpRequired] = useState(false)
  const [totpCode, setTotpCode] = useState('')
  const [forgotMode, setForgotMode] = useState(false)
  const [forgotAddress, setForgotAddress] = useState('')
  const [forgotRecoveryEmail, setForgotRecoveryEmail] = useState('')
  const [forgotLoading, setForgotLoading] = useState(false)
  const [forgotMessage, setForgotMessage] = useState('')
  // whom to ask for a reset code, from a sign-in to a cleared account
  const [cleared, setCleared] = useState<null | ResetContact>(null)
  const [notice, setNotice] = useState('')
  const resetMode = searchParams.get('mode') === 'reset-code'

  const openReset = () => {
    setError('')
    setSearchParams({ mode: 'reset-code' })
  }
  const closeReset = () => setSearchParams({})

  // fetch OIDC config
  useEffect(() => {
    wireGetOidcConfig()
      .then((cfg) => setOidcConfig(cfg as unknown as OidcClientConfig))
      .catch(() => {})
  }, [])

  // Which third-party sign-ins this deployment actually has credentials for.
  // An unconfigured provider is absent rather than a button leading nowhere.
  useEffect(() => {
    wireExternalProviders()
      .then((keys) => setExternalProviders([...keys]))
      .catch(() => {})
  }, [])

  // What the callback sent us back with. `link` is the one that matters:
  // the identity authenticated but belongs to no mailbox yet, so the
  // password below is what will attach it.
  const externalResult = searchParams.get('external')

  // A third-party sign-in finished: the callback set the session cookie and
  // sent the browser here. The profile comes from the cookie, never the URL.
  useEffect(() => {
    if (externalResult !== 'ok') return
    wireGetMe()
      .then((me) => {
        setAuth({
          accessible_domains: me.accessible_domains,
          address: me.address,
          display_name: me.display_name,
          is_admin: me.is_admin,
          is_super: me.is_super,
          permissions: me.permissions,
        })
        navigate('/', { replace: true })
      })
      .catch(() => navigate('/login?external=failed', { replace: true }))
  }, [externalResult, setAuth, navigate])

  const handleForgotPassword = async (e: React.FormEvent) => {
    e.preventDefault()
    setForgotLoading(true)
    setForgotMessage('')
    try {
      await wireForgotPassword(forgotAddress, forgotRecoveryEmail || undefined)
      setForgotMessage(t('login.resetLinkSent'))
    } catch {
      setForgotMessage(t('login.resetLinkSent'))
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
        is_admin: data.is_admin,
        is_super: data.is_super,
        permissions: data.permissions,
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
      if (e instanceof WireErrorException && e.detail.kind === 'password-cleared') {
        setCleared(e.detail.contact)
        openReset()
        return
      }
      setError(loginErrorText(e, t))
    } finally {
      setLoading(false)
    }
  }

  if (resetMode) {
    return (
      <ResetWithCode
        address={address}
        contact={cleared}
        onCancel={closeReset}
        onDone={(done) => {
          setAddress(done)
          setPassword('')
          setNotice(t('login.newPasswordSet'))
          closeReset()
        }}
      />
    )
  }

  if (forgotMode) {
    return (
      <AuthCard onSubmit={handleForgotPassword}>
        <BrandHeader subtitle={t('login.resetTitle')} />

        {forgotMessage && (
          <Alert role="status" variant="success">
            {forgotMessage}
          </Alert>
        )}

        <AuthField
          autoComplete="email"
          autoFocus
          id="forgot-email"
          label={t('login.accountEmail')}
          onChange={setForgotAddress}
          placeholder="you@domain.com"
          required
          type="email"
          value={forgotAddress}
        />

        <AuthField
          autoComplete="email"
          helperText={t('login.recoveryHelp')}
          id="forgot-recovery"
          label={t('login.recoveryEmail')}
          onChange={setForgotRecoveryEmail}
          placeholder={t('login.recoveryPlaceholder')}
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
          {t(resetButtonLabel(forgotLoading))}
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
            {t('login.backToLogin')}
          </button>
        </div>
      </AuthCard>
    )
  }

  return (
    <AuthCard onSubmit={handleSubmit}>
      <BrandHeader subtitle={t('login.title')} />

      {notice && (
        <Alert role="status" variant="success">
          {notice}
        </Alert>
      )}

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
            label={t('login.email')}
            onChange={setAddress}
            placeholder="you@domain.com"
            required
            type="email"
            value={address}
          />

          <AuthField
            autoComplete="current-password"
            id="login-password"
            label={t('login.password')}
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
          helperText={t('login.totpHelp')}
          id="login-totp"
          inputMode="numeric"
          label={t('login.totpLabel')}
          onChange={setTotpCode}
          placeholder={t('login.totpPlaceholder')}
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
          <span className="text-fg-secondary text-sm">{t('login.rememberEmail')}</span>
        </label>
      )}

      <Button disabled={loading} fullWidth loading={loading} type="submit" variant="primary">
        {t(submitLabel(loading, totpRequired))}
      </Button>

      <div className="flex justify-center gap-4">
        <button
          className="text-accent text-sm hover:underline"
          onClick={() => {
            setForgotMode(true)
            setError('')
            setForgotMessage('')
          }}
          type="button"
        >
          {t('login.forgotPassword')}
        </button>
        <button className="text-accent text-sm hover:underline" onClick={openReset} type="button">
          {t('login.haveResetCode')}
        </button>
      </div>

      {externalResult && EXTERNAL_MESSAGES[externalResult] && (
        <Alert
          role="status"
          variant={EXTERNAL_MESSAGES[externalResult].tone === 'info' ? 'info' : 'danger'}
        >
          {t(EXTERNAL_MESSAGES[externalResult].text)}
        </Alert>
      )}

      {(oidcConfig?.enabled || externalProviders.length > 0) && (
        <>
          <div className="flex items-center gap-3">
            <div className="bg-border h-px flex-1" />
            <span className="text-fg-muted text-xs">{t('login.or')}</span>
            <div className="bg-border h-px flex-1" />
          </div>
          {oidcConfig?.enabled && (
            <a
              className="border-border bg-bg-secondary text-fg hover:bg-bg-secondary flex w-full items-center justify-center gap-2 rounded-md border px-3 py-2 text-sm font-medium transition-colors"
              href={oidcConfig.login_url}
            >
              <ExternalLink className="h-4 w-4" />
              {t('login.signInWith', { provider: oidcConfig.provider_name ?? 'SSO' })}
            </a>
          )}
          {externalProviders.map((key) => (
            <a
              className="border-border bg-bg-secondary text-fg hover:bg-bg-secondary flex w-full items-center justify-center gap-2 rounded-md border px-3 py-2 text-sm font-medium transition-colors"
              href={`/api/auth/external/${encodeURIComponent(key)}`}
              key={key}
            >
              <ExternalLink className="h-4 w-4" />
              {t('login.signInWith', { provider: PROVIDER_LABELS[key] ?? key })}
            </a>
          ))}
        </>
      )}
    </AuthCard>
  )
}

function loginErrorText(e: unknown, t: (key: Key) => string): string {
  if (!(e instanceof WireErrorException)) return t('compose.networkError')
  if (e.detail.kind === 'server') return e.detail.message ?? t('login.failed')
  if (e.detail.kind === 'network') return t('compose.networkError')
  return t('login.failed')
}

function resetButtonLabel(sending: boolean): Key {
  if (sending) return 'compose.sending'
  return 'login.sendResetLink'
}

function submitLabel(loading: boolean, totpRequired: boolean): Key {
  if (loading) return 'login.signingIn'
  if (totpRequired) return 'login.verify'
  return 'login.signIn'
}
