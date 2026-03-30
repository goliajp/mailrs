import type { ThemeMode } from '@goliapkg/gds'

import { toast } from '@goliapkg/gds'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useState } from 'react'

import { deleteJson, fetchJson, postJson, putJson } from '@/lib/api'
import { authAtom, getToken } from '@/store/auth'
import {
  notificationsAtom,
  notificationSoundAtom,
  pageSizeAtom,
} from '@/store/settings'
import { themeAtom } from '@/store/theme'

// --- types ---

type AgentKey = {
  created_at: string
  expires_at: null | string
  id: string
  name: string
  prefix: string
}

type Category =
  | 'account'
  | 'api-keys'
  | 'appearance'
  | 'keys'
  | 'security'
  | 'signatures'
  | 'webhooks'

type CreatedAgentKey = {
  id: string
  key: string
  prefix: string
}

type CreatedWebhook = {
  id: string
  signing_secret: string
}

type KeyStatus = {
  pgp_fingerprint: null | string
  smime_fingerprint: null | string
}

type Signature = {
  html_content: string
  id: number
  is_default: boolean
  name: string
  text_content: string
}

type TotpSetup = {
  qr_url: string
  recovery_codes: string[]
  secret: string
}

type TotpStatus = {
  enabled: boolean
}

type Webhook = {
  active: boolean
  created_at: string
  event_type: string
  filter_sender: null | string
  filter_thread_id: null | string
  id: string
  url: string
}

// --- constants ---

const CATEGORIES: { key: Category; label: string }[] = [
  { key: 'account', label: 'Account' },
  { key: 'security', label: 'Security' },
  { key: 'signatures', label: 'Signatures' },
  { key: 'keys', label: 'Encryption Keys' },
  { key: 'api-keys', label: 'API Keys' },
  { key: 'webhooks', label: 'Webhooks' },
  { key: 'appearance', label: 'Appearance' },
]

const THEME_OPTIONS: { label: string; value: ThemeMode }[] = [
  { label: 'Light', value: 'light' },
  { label: 'Dark', value: 'dark' },
  { label: 'System', value: 'system' },
]

const PAGE_SIZE_OPTIONS = [20, 50, 100, 200]

const inputClass =
  'w-full rounded-md border border-border bg-bg-secondary px-3 py-1.5 text-sm focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent/40'
const btnPrimary =
  'rounded-md bg-fg px-3 py-1.5 text-sm font-medium text-bg transition-colors hover:opacity-90 disabled:opacity-50'
const btnDanger =
  'rounded-md bg-danger px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50'
const btnSecondary =
  'rounded-md px-3 py-1.5 text-sm text-fg-secondary transition-colors hover:bg-bg-secondary'
const cardClass = 'rounded-lg border border-border p-4'

// --- main component ---

export function Settings() {
  const [active, setActive] = useState<Category>('account')

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <header className="border-border border-b px-4 py-3 sm:px-6">
        <h1 className="text-lg font-semibold tracking-tight">Settings</h1>
      </header>

      <div className="flex min-h-0 flex-1 flex-col sm:flex-row">
        {/* sidebar - horizontal on mobile, vertical on desktop */}
        <nav className="border-border flex shrink-0 gap-1 overflow-x-auto border-b p-2 sm:w-48 sm:flex-col sm:overflow-x-visible sm:overflow-y-auto sm:border-r sm:border-b-0 sm:p-3">
          {CATEGORIES.map((cat) => (
            <button
              className={`rounded-md px-3 py-1.5 text-left text-sm whitespace-nowrap transition-colors ${
                active === cat.key
                  ? 'bg-accent text-accent-fg'
                  : 'text-fg-secondary hover:bg-bg-secondary'
              }`}
              key={cat.key}
              onClick={() => setActive(cat.key)}
            >
              {cat.label}
            </button>
          ))}
        </nav>

        {/* content panel */}
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-6 sm:px-8">
          <div className="mx-auto max-w-2xl">
            {active === 'account' && <AccountSection />}
            {active === 'security' && <SecuritySection />}
            {active === 'signatures' && <SignaturesSection />}
            {active === 'keys' && <EncryptionKeysSection />}
            {active === 'api-keys' && <ApiKeysSection />}
            {active === 'webhooks' && <WebhooksSection />}
            {active === 'appearance' && <AppearanceSection />}
          </div>
        </div>
      </div>
    </div>
  )
}

// --- account section ---

function AccountSection() {
  const auth = useAtomValue(authAtom)
  const setAuth = useSetAtom(authAtom)
  const [pw, setPw] = useState({ confirm: '', current: '', next: '' })
  const [saving, setSaving] = useState(false)
  const [recoveryEmail, setRecoveryEmail] = useState('')
  const [recoveryLoaded, setRecoveryLoaded] = useState(false)
  const [savingRecovery, setSavingRecovery] = useState(false)

  useEffect(() => {
    fetchJson<{ recovery_email: string }>('/auth/recovery-email')
      .then((data) => {
        setRecoveryEmail(data.recovery_email)
        setRecoveryLoaded(true)
      })
      .catch(() => setRecoveryLoaded(true))
  }, [])

  const handleRecoveryEmailSave = async () => {
    setSavingRecovery(true)
    try {
      await postJson('/auth/recovery-email', { recovery_email: recoveryEmail })
      toast.success('Recovery email updated')
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : 'Failed to update recovery email'
      )
    } finally {
      setSavingRecovery(false)
    }
  }

  const handlePasswordChange = async () => {
    if (!pw.current || !pw.next) return
    if (pw.next !== pw.confirm) {
      toast.error('New passwords do not match')
      return
    }
    if (pw.next.length < 8) {
      toast.error('Password must be at least 8 characters')
      return
    }
    setSaving(true)
    try {
      await postJson('/auth/change-password', {
        current_password: pw.current,
        new_password: pw.next,
      })
      toast.success('Password updated')
      setPw({ confirm: '', current: '', next: '' })
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update password')
    } finally {
      setSaving(false)
    }
  }

  const [showLogoutConfirm, setShowLogoutConfirm] = useState(false)

  const doLogout = async () => {
    try {
      await postJson('/auth/logout', {})
    } catch {
      // ignore
    }
    setAuth(null)
    window.location.href = '/login'
  }

  return (
    <div className="space-y-6">
      <SectionHeader title="Account" />

      <div className={cardClass}>
        <div className="space-y-3">
          <Field label="Email">
            <span className="text-fg-secondary text-sm">
              {auth?.address ?? '-'}
            </span>
          </Field>
          <Field label="Display Name">
            <span className="text-fg-secondary text-sm">
              {auth?.display_name || '-'}
            </span>
          </Field>
          <Field label="Permissions">
            <span className="text-fg-secondary text-sm">
              {auth?.permissions?.join(', ') || '-'}
            </span>
          </Field>
          <Field label="Domains">
            <span className="text-fg-secondary text-sm">
              {auth?.accessible_domains?.join(', ') || '-'}
            </span>
          </Field>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-3 text-sm font-medium">Recovery Email</h3>
        <p className="text-fg-muted mb-2 text-xs">
          Used to receive password reset links. Must be an external email
          address you can access independently.
        </p>
        <div className="flex gap-2">
          <input
            className={inputClass + ' flex-1'}
            disabled={!recoveryLoaded}
            onChange={(e) => setRecoveryEmail(e.target.value)}
            placeholder={recoveryLoaded ? 'Not configured' : 'Loading...'}
            type="email"
            value={recoveryEmail}
          />
          <button
            className={btnPrimary}
            disabled={savingRecovery || !recoveryLoaded}
            onClick={handleRecoveryEmailSave}
          >
            {savingRecovery ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-3 text-sm font-medium">Change Password</h3>
        <div className="space-y-2">
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-current-pw"
            >
              Current password
            </label>
            <input
              className={inputClass}
              id="settings-current-pw"
              onChange={(e) => setPw({ ...pw, current: e.target.value })}
              placeholder="Current password"
              type="password"
              value={pw.current}
            />
          </div>
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-new-pw"
            >
              New password
            </label>
            <input
              className={inputClass}
              id="settings-new-pw"
              onChange={(e) => setPw({ ...pw, next: e.target.value })}
              placeholder="New password"
              type="password"
              value={pw.next}
            />
          </div>
          <div>
            <label
              className="text-fg-secondary mb-1 block text-xs font-medium"
              htmlFor="settings-confirm-pw"
            >
              Confirm new password
            </label>
            <input
              className={inputClass}
              id="settings-confirm-pw"
              onChange={(e) => setPw({ ...pw, confirm: e.target.value })}
              placeholder="Confirm new password"
              type="password"
              value={pw.confirm}
            />
          </div>
          <button
            className={btnPrimary}
            disabled={saving}
            onClick={handlePasswordChange}
          >
            {saving ? 'Saving...' : 'Update Password'}
          </button>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-2 text-sm font-medium">Export Mailbox</h3>
        <p className="text-fg-muted mb-3 text-xs">
          Download all your emails as an MBOX file
        </p>
        <button
          className={btnPrimary}
          onClick={() =>
            window.open(`/api/mail/export?token=${getToken()}`, '_blank')
          }
        >
          Export as MBOX
        </button>
      </div>

      <div className="border-border border-t pt-4">
        <button
          className={btnDanger}
          onClick={() => setShowLogoutConfirm(true)}
        >
          Sign out
        </button>
        {auth?.address && (
          <p className="text-fg-muted mt-2 text-xs">
            Signed in as {auth.address}
          </p>
        )}
      </div>

      {showLogoutConfirm && (
        <ConfirmDialog
          message="Sign out? You will need to sign in again to access your mailbox."
          onCancel={() => setShowLogoutConfirm(false)}
          onConfirm={doLogout}
        />
      )}
    </div>
  )
}

// --- security section ---

function ApiKeysSection() {
  const [keys, setKeys] = useState<AgentKey[]>([])
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({ expires_in_days: '', name: '' })
  const [createdKey, setCreatedKey] = useState<CreatedAgentKey | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<AgentKey[]>('/agent/keys')
      setKeys(data)
    } catch {
      // keep current
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const handleCreate = async () => {
    if (!form.name.trim()) return
    try {
      const expires_in_days = form.expires_in_days
        ? parseInt(form.expires_in_days, 10)
        : undefined
      const data = await postJson<CreatedAgentKey>('/agent/keys', {
        expires_in_days,
        name: form.name.trim(),
      })
      toast.success('API key created')
      setCreatedKey(data)
      setForm({ expires_in_days: '', name: '' })
      setAdding(false)
      load()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create key')
    }
  }

  const handleRevoke = async (id: string) => {
    try {
      await deleteJson(`/agent/keys/${id}`)
      toast.success('API key revoked')
      setDeleteTarget(null)
      load()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to revoke key')
      setDeleteTarget(null)
    }
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard
      .writeText(text)
      .then(() => toast.success('Copied to clipboard'))
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title="API Keys" />
        <button className={btnPrimary} onClick={() => setAdding(true)}>
          Create Key
        </button>
      </div>

      {createdKey && (
        <div className="border-warning bg-warning/10 rounded-lg border p-4">
          <p className="mb-2 text-sm font-semibold">API Key Created</p>
          <p className="text-fg-secondary mb-2 text-xs">
            Copy this key now. It will not be shown again.
          </p>
          <div className="flex items-center gap-2">
            <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm">
              {createdKey.key}
            </code>
            <button
              className={btnPrimary}
              onClick={() => copyToClipboard(createdKey.key)}
            >
              Copy
            </button>
          </div>
          <button
            className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
            onClick={() => setCreatedKey(null)}
          >
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <div className={cardClass + ' space-y-3'}>
          <input
            className={inputClass}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="Key name"
            value={form.name}
          />
          <input
            className={inputClass}
            min="1"
            onChange={(e) =>
              setForm({ ...form, expires_in_days: e.target.value })
            }
            placeholder="Expires in days (optional)"
            type="number"
            value={form.expires_in_days}
          />
          <div className="flex gap-2">
            <button className={btnPrimary} onClick={handleCreate}>
              Create
            </button>
            <button className={btnSecondary} onClick={() => setAdding(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {keys.length === 0 && !adding && (
        <p className="text-fg-muted text-sm">No API keys</p>
      )}

      {keys.map((k) => (
        <div className={cardClass} key={k.id}>
          <div className="flex items-center justify-between">
            <div>
              <span className="text-sm font-medium">{k.name}</span>
              <span className="text-fg-muted ml-2 font-mono text-xs">
                {k.prefix}...
              </span>
              {k.expires_at && (
                <span className="text-fg-muted ml-2 text-xs">
                  expires {new Date(k.expires_at).toLocaleDateString()}
                </span>
              )}
            </div>
            <button
              className="text-danger text-xs"
              onClick={() => setDeleteTarget(k.id)}
            >
              Revoke
            </button>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Revoke this API key? This cannot be undone."
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleRevoke(deleteTarget)}
        />
      )}
    </div>
  )
}

// --- signatures section ---

function AppearanceSection() {
  const [theme, setTheme] = useAtom(themeAtom)
  const [pageSize, setPageSize] = useAtom(pageSizeAtom)
  const [notifications, setNotifications] = useAtom(notificationsAtom)
  const [notificationSound, setNotificationSound] = useAtom(
    notificationSoundAtom
  )
  const [notificationError, setNotificationError] = useState<null | string>(
    null
  )

  const handleNotificationToggle = useCallback(
    async (enabled: boolean) => {
      if (typeof Notification === 'undefined') {
        setNotificationError(
          'Browser notifications are not supported on this device.'
        )
        return
      }
      if (enabled && Notification.permission === 'default') {
        const result = await Notification.requestPermission()
        if (result === 'denied') {
          setNotificationError(
            'Browser notifications were denied. Please enable them in your browser settings.'
          )
          return
        }
      }
      if (enabled && Notification.permission === 'denied') {
        setNotificationError(
          'Browser notifications are blocked. Please enable them in your browser settings.'
        )
        return
      }
      setNotificationError(null)
      setNotifications(enabled)
    },
    [setNotifications]
  )

  return (
    <div className="space-y-6">
      <SectionHeader title="Appearance" />

      <div className={cardClass}>
        <Field label="Theme">
          <div className="flex gap-1">
            {THEME_OPTIONS.map((opt) => (
              <button
                className={`rounded-md px-3 py-1.5 text-sm transition-colors ${
                  theme === opt.value
                    ? 'bg-accent text-accent-fg'
                    : 'bg-surface text-fg-secondary hover:bg-bg-secondary'
                }`}
                key={opt.value}
                onClick={() => setTheme(opt.value)}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </Field>
      </div>

      <div className={cardClass}>
        <Field label="Page size">
          <select
            className="border-border bg-surface text-fg-secondary focus:border-accent focus:ring-accent/40 rounded-md border px-3 py-1.5 text-sm focus:ring-1 focus:outline-none"
            onChange={(e) => setPageSize(Number(e.target.value))}
            value={pageSize}
          >
            {PAGE_SIZE_OPTIONS.map((size) => (
              <option key={size} value={size}>
                {size} per page
              </option>
            ))}
          </select>
        </Field>
      </div>

      <div className={cardClass}>
        <div className="space-y-3">
          <Field label="Browser notifications">
            <div className="flex flex-col items-end gap-1">
              <Toggle
                checked={notifications}
                onChange={(v) => handleNotificationToggle(v)}
              />
              {notificationError && (
                <p className="text-danger text-xs">{notificationError}</p>
              )}
            </div>
          </Field>
          <Field label="Notification sound">
            <Toggle
              checked={notificationSound}
              onChange={setNotificationSound}
            />
          </Field>
        </div>
      </div>
    </div>
  )
}

// --- encryption keys section ---

function ConfirmDialog({
  message,
  onCancel,
  onConfirm,
}: {
  message: string
  onCancel: () => void
  onConfirm: () => void
}) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onCancel])

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onCancel}
    >
      <div
        className="bg-surface w-full max-w-sm rounded-lg p-6 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <p className="text-fg-secondary mb-4 text-sm">{message}</p>
        <div className="flex justify-end gap-2">
          <button className={btnSecondary} onClick={onCancel}>
            Cancel
          </button>
          <button className={btnDanger} onClick={onConfirm}>
            Confirm
          </button>
        </div>
      </div>
    </div>
  )
}

// --- api keys section ---

function EncryptionKeysSection() {
  const [status, setStatus] = useState<KeyStatus | null>(null)
  const [pgpKey, setPgpKey] = useState('')
  const [smimeCert, setSmimeCert] = useState('')
  const [saving, setSaving] = useState<null | string>(null)
  const [deleteKeyTarget, setDeleteKeyTarget] = useState<
    'pgp' | 'smime' | null
  >(null)

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<KeyStatus>('/mail/keys/status')
      setStatus(data)
    } catch {
      // keep null
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const handleUpload = async (type: 'pgp' | 'smime', content: string) => {
    if (!content.trim()) return
    setSaving(type)
    try {
      await putJson(`/mail/keys/${type}`, { content: content.trim() })
      toast.success(`${type.toUpperCase()} key saved`)
      if (type === 'pgp') {
        setPgpKey('')
      } else {
        setSmimeCert('')
      }
      load()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : `Failed to save ${type} key`)
    } finally {
      setSaving(null)
    }
  }

  const handleDelete = async (type: 'pgp' | 'smime') => {
    setSaving(type)
    try {
      await deleteJson(`/mail/keys/${type}`)
      toast.success(`${type.toUpperCase()} key deleted`)
      load()
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : `Failed to delete ${type} key`
      )
    } finally {
      setSaving(null)
    }
  }

  return (
    <div className="space-y-6">
      <SectionHeader title="Encryption Keys" />

      <div className={cardClass + ' space-y-4'}>
        <h3 className="text-sm font-medium">PGP Key</h3>
        <Field label="Fingerprint">
          <span className="text-fg-secondary font-mono text-xs">
            {status?.pgp_fingerprint ?? 'Not configured'}
          </span>
        </Field>
        <textarea
          className={inputClass}
          onChange={(e) => setPgpKey(e.target.value)}
          placeholder="Paste PGP public key (ASCII-armored)"
          rows={4}
          value={pgpKey}
        />
        <div className="flex gap-2">
          <button
            className={btnPrimary}
            disabled={saving === 'pgp'}
            onClick={() => handleUpload('pgp', pgpKey)}
          >
            {saving === 'pgp' ? 'Saving...' : 'Save PGP Key'}
          </button>
          {status?.pgp_fingerprint && (
            <button
              className={btnDanger}
              disabled={saving === 'pgp'}
              onClick={() => setDeleteKeyTarget('pgp')}
            >
              Delete
            </button>
          )}
        </div>
      </div>

      <div className={cardClass + ' space-y-4'}>
        <h3 className="text-sm font-medium">S/MIME Certificate</h3>
        <Field label="Fingerprint">
          <span className="text-fg-secondary font-mono text-xs">
            {status?.smime_fingerprint ?? 'Not configured'}
          </span>
        </Field>
        <textarea
          className={inputClass}
          onChange={(e) => setSmimeCert(e.target.value)}
          placeholder="Paste S/MIME certificate (PEM format)"
          rows={4}
          value={smimeCert}
        />
        <div className="flex gap-2">
          <button
            className={btnPrimary}
            disabled={saving === 'smime'}
            onClick={() => handleUpload('smime', smimeCert)}
          >
            {saving === 'smime' ? 'Saving...' : 'Save S/MIME Cert'}
          </button>
          {status?.smime_fingerprint && (
            <button
              className={btnDanger}
              disabled={saving === 'smime'}
              onClick={() => setDeleteKeyTarget('smime')}
            >
              Delete
            </button>
          )}
        </div>
      </div>

      {deleteKeyTarget && (
        <ConfirmDialog
          message={`Delete your ${deleteKeyTarget.toUpperCase()} key? This cannot be undone.`}
          onCancel={() => setDeleteKeyTarget(null)}
          onConfirm={() => {
            handleDelete(deleteKeyTarget)
            setDeleteKeyTarget(null)
          }}
        />
      )}
    </div>
  )
}

// --- webhooks section ---

function Field({
  children,
  label,
}: {
  children: React.ReactNode
  label: string
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-fg-secondary text-sm font-medium">{label}</span>
      {children}
    </div>
  )
}

// --- appearance section ---

function SectionHeader({ title }: { title: string }) {
  return <h2 className="mb-4 text-base font-semibold">{title}</h2>
}

// --- shared ui components ---

function SecuritySection() {
  const [status, setStatus] = useState<null | TotpStatus>(null)
  const [setup, setSetup] = useState<null | TotpSetup>(null)
  const [code, setCode] = useState('')
  const [loading, setLoading] = useState(true)
  const [submitting, setSubmitting] = useState(false)

  const loadStatus = useCallback(async () => {
    try {
      const data = await fetchJson<TotpStatus>('/auth/totp/status')
      setStatus(data)
    } catch {
      // keep null
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadStatus()
  }, [loadStatus])

  const handleSetup = async () => {
    try {
      const data = await postJson<TotpSetup>('/auth/totp/setup', {})
      setSetup(data)
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to set up 2FA')
    }
  }

  const handleEnable = async () => {
    if (!code.trim()) return
    setSubmitting(true)
    try {
      await postJson('/auth/totp/enable', { code: code.trim() })
      toast.success('2FA enabled')
      setSetup(null)
      setCode('')
      loadStatus()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Invalid code')
    } finally {
      setSubmitting(false)
    }
  }

  const handleDisable = async () => {
    if (!code.trim()) return
    setSubmitting(true)
    try {
      await postJson('/auth/totp/disable', { code: code.trim() })
      toast.success('2FA disabled')
      setCode('')
      loadStatus()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Invalid code')
    } finally {
      setSubmitting(false)
    }
  }

  if (loading)
    return (
      <div className="text-fg-muted flex items-center gap-2 py-4 text-sm">
        <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
        Loading...
      </div>
    )

  return (
    <div className="space-y-6">
      <SectionHeader title="Two-Factor Authentication" />

      <div className={cardClass}>
        <Field label="Status">
          <span
            className={`rounded-full px-2 py-0.5 text-xs font-medium ${
              status?.enabled
                ? 'bg-success/10 text-success'
                : 'bg-surface text-fg-muted'
            }`}
          >
            {status?.enabled ? 'Enabled' : 'Disabled'}
          </span>
        </Field>
      </div>

      {!status?.enabled && !setup && (
        <button className={btnPrimary} onClick={handleSetup}>
          Set up 2FA
        </button>
      )}

      {setup && (
        <div className={cardClass + ' space-y-4'}>
          <h3 className="text-sm font-medium">Scan QR Code</h3>
          <div className="bg-surface rounded-md p-3">
            <p className="text-fg-secondary font-mono text-xs break-all">
              {setup.qr_url}
            </p>
          </div>

          {setup.recovery_codes.length > 0 && (
            <div className="border-warning bg-warning/10 rounded-lg border p-3">
              <p className="mb-2 text-sm font-semibold">Recovery Codes</p>
              <p className="text-fg-secondary mb-2 text-xs">
                Save these codes in a safe place. Each code can only be used
                once.
              </p>
              <div className="grid grid-cols-2 gap-1">
                {setup.recovery_codes.map((rc) => (
                  <code
                    className="bg-bg-secondary rounded px-2 py-1 font-mono text-xs"
                    key={rc}
                  >
                    {rc}
                  </code>
                ))}
              </div>
            </div>
          )}

          <div className="flex gap-2">
            <input
              className={inputClass + ' max-w-[200px]'}
              onChange={(e) => setCode(e.target.value)}
              placeholder="Enter TOTP code"
              value={code}
            />
            <button
              className={btnPrimary}
              disabled={submitting}
              onClick={handleEnable}
            >
              {submitting ? 'Verifying...' : 'Verify & Enable'}
            </button>
          </div>
        </div>
      )}

      {status?.enabled && (
        <div className={cardClass + ' space-y-3'}>
          <h3 className="text-sm font-medium">Disable 2FA</h3>
          <div className="flex gap-2">
            <input
              className={inputClass + ' max-w-[200px]'}
              onChange={(e) => setCode(e.target.value)}
              placeholder="Enter TOTP code"
              value={code}
            />
            <button
              className={btnDanger}
              disabled={submitting}
              onClick={handleDisable}
            >
              {submitting ? 'Disabling...' : 'Disable 2FA'}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

function SignaturesSection() {
  const [signatures, setSignatures] = useState<Signature[]>([])
  const [editing, setEditing] = useState<null | Partial<Signature>>(null)
  const [saving, setSaving] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<Signature[]>('/mail/signatures')
      setSignatures(data)
    } catch {
      // keep current
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const handleSave = async () => {
    if (!editing?.name?.trim()) return
    setSaving(true)
    try {
      await postJson('/mail/signatures', {
        html_content: editing.html_content ?? '',
        id: editing.id,
        is_default: editing.is_default ?? false,
        name: editing.name.trim(),
        text_content: editing.text_content ?? '',
      })
      toast.success(editing.id ? 'Signature updated' : 'Signature created')
      setEditing(null)
      load()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to save signature')
    } finally {
      setSaving(false)
    }
  }

  const handleDelete = async (id: number) => {
    try {
      await deleteJson(`/mail/signatures/${id}`)
      toast.success('Signature deleted')
      setDeleteTarget(null)
      load()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete signature')
      setDeleteTarget(null)
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title="Signatures" />
        <button
          className={btnPrimary}
          onClick={() =>
            setEditing({
              html_content: '',
              is_default: false,
              name: '',
              text_content: '',
            })
          }
        >
          Add Signature
        </button>
      </div>

      {editing && (
        <div className={cardClass + ' space-y-3'}>
          <input
            className={inputClass}
            onChange={(e) => setEditing({ ...editing, name: e.target.value })}
            placeholder="Signature name"
            value={editing.name ?? ''}
          />
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">
              Text content
            </label>
            <textarea
              className={inputClass}
              onChange={(e) =>
                setEditing({ ...editing, text_content: e.target.value })
              }
              rows={3}
              value={editing.text_content ?? ''}
            />
          </div>
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">
              HTML content
            </label>
            <textarea
              className={inputClass}
              onChange={(e) =>
                setEditing({ ...editing, html_content: e.target.value })
              }
              rows={3}
              value={editing.html_content ?? ''}
            />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input
              checked={editing.is_default ?? false}
              onChange={(e) =>
                setEditing({ ...editing, is_default: e.target.checked })
              }
              type="checkbox"
            />
            Default signature
          </label>
          <div className="flex gap-2">
            <button
              className={btnPrimary}
              disabled={saving}
              onClick={handleSave}
            >
              {saving ? 'Saving...' : 'Save'}
            </button>
            <button className={btnSecondary} onClick={() => setEditing(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {signatures.length === 0 && !editing && (
        <p className="text-fg-muted text-sm">No signatures configured</p>
      )}

      {signatures.map((sig) => (
        <div className={cardClass} key={sig.id}>
          <div className="flex items-start justify-between">
            <div>
              <span className="text-sm font-medium">{sig.name}</span>
              {sig.is_default && (
                <span className="bg-success/10 text-success ml-2 rounded-full px-2 py-0.5 text-xs">
                  Default
                </span>
              )}
              {sig.text_content && (
                <p className="text-fg-muted mt-1 text-xs whitespace-pre-wrap">
                  {sig.text_content.slice(0, 120)}
                  {sig.text_content.length > 120 ? '...' : ''}
                </p>
              )}
            </div>
            <div className="flex gap-2">
              <button
                className="text-accent text-xs"
                onClick={() => setEditing(sig)}
              >
                Edit
              </button>
              <button
                className="text-danger text-xs"
                onClick={() => setDeleteTarget(sig.id)}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Delete this signature? This cannot be undone."
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  )
}

function Toggle({
  checked,
  onChange,
}: {
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <button
      aria-checked={checked}
      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
        checked ? 'bg-accent' : 'bg-border-strong'
      }`}
      onClick={() => onChange(!checked)}
      role="switch"
    >
      <span
        className={`inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
          checked ? 'translate-x-6' : 'translate-x-1'
        }`}
      />
    </button>
  )
}

function WebhooksSection() {
  const [webhooks, setWebhooks] = useState<Webhook[]>([])
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({
    event_type: 'new_message',
    filter_sender: '',
    filter_thread_id: '',
    url: '',
  })
  const [createdSecret, setCreatedSecret] = useState<null | string>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<Webhook[]>('/agent/webhooks')
      setWebhooks(data)
    } catch {
      // keep current
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const [creating, setCreating] = useState(false)
  const handleCreate = async () => {
    if (!form.url.trim() || creating) return
    setCreating(true)
    try {
      const data = await postJson<CreatedWebhook>('/agent/webhooks', {
        event_type: form.event_type,
        url: form.url.trim(),
        ...(form.filter_sender.trim()
          ? { filter_sender: form.filter_sender.trim() }
          : {}),
        ...(form.filter_thread_id.trim()
          ? { filter_thread_id: form.filter_thread_id.trim() }
          : {}),
      })
      toast.success('Webhook created')
      setCreatedSecret(data.signing_secret)
      setForm({
        event_type: 'new_message',
        filter_sender: '',
        filter_thread_id: '',
        url: '',
      })
      setAdding(false)
      load()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create webhook')
    } finally {
      setCreating(false)
    }
  }

  const handleDelete = async (id: string) => {
    try {
      await deleteJson(`/agent/webhooks/${id}`)
      toast.success('Webhook deleted')
      setDeleteTarget(null)
      load()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete webhook')
      setDeleteTarget(null)
    }
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard
      .writeText(text)
      .then(() => toast.success('Copied to clipboard'))
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title="Webhooks" />
        <button className={btnPrimary} onClick={() => setAdding(true)}>
          Add Webhook
        </button>
      </div>

      {createdSecret && (
        <div className="border-warning bg-warning/10 rounded-lg border p-4">
          <p className="mb-2 text-sm font-semibold">Signing Secret</p>
          <p className="text-fg-secondary mb-2 text-xs">
            Copy this secret now. It will not be shown again.
          </p>
          <div className="flex items-center gap-2">
            <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm">
              {createdSecret}
            </code>
            <button
              className={btnPrimary}
              onClick={() => copyToClipboard(createdSecret)}
            >
              Copy
            </button>
          </div>
          <button
            className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
            onClick={() => setCreatedSecret(null)}
          >
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <div className={cardClass + ' space-y-3'}>
          <input
            className={inputClass}
            onChange={(e) => setForm({ ...form, url: e.target.value })}
            placeholder="https://example.com/webhook"
            type="url"
            value={form.url}
          />
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">
              Event type
            </label>
            <select
              className={inputClass}
              onChange={(e) => setForm({ ...form, event_type: e.target.value })}
              value={form.event_type}
            >
              <option value="new_message">new_message</option>
              <option value="message_read">message_read</option>
              <option value="message_deleted">message_deleted</option>
            </select>
          </div>
          <input
            className={inputClass}
            onChange={(e) =>
              setForm({ ...form, filter_sender: e.target.value })
            }
            placeholder="Filter by sender (optional)"
            value={form.filter_sender}
          />
          <input
            className={inputClass}
            onChange={(e) =>
              setForm({ ...form, filter_thread_id: e.target.value })
            }
            placeholder="Filter by thread ID (optional)"
            value={form.filter_thread_id}
          />
          <div className="flex gap-2">
            <button
              className={btnPrimary}
              disabled={creating}
              onClick={handleCreate}
            >
              {creating ? 'Creating…' : 'Create'}
            </button>
            <button className={btnSecondary} onClick={() => setAdding(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {webhooks.length === 0 && !adding && (
        <p className="text-fg-muted text-sm">No webhooks configured</p>
      )}

      {webhooks.map((wh) => (
        <div className={cardClass} key={wh.id}>
          <div className="flex items-start justify-between">
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium" title={wh.url}>
                {wh.url}
              </p>
              <div className="mt-1 flex flex-wrap gap-2">
                <span className="bg-accent/10 text-accent rounded-full px-2 py-0.5 text-xs">
                  {wh.event_type}
                </span>
                {wh.filter_sender && (
                  <span className="bg-surface text-fg-muted rounded-full px-2 py-0.5 text-xs">
                    sender: {wh.filter_sender}
                  </span>
                )}
                {wh.filter_thread_id && (
                  <span className="bg-surface text-fg-muted rounded-full px-2 py-0.5 text-xs">
                    thread: {wh.filter_thread_id}
                  </span>
                )}
                <span
                  className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                    wh.active
                      ? 'bg-success/10 text-success'
                      : 'bg-surface text-fg-muted'
                  }`}
                >
                  {wh.active ? 'Active' : 'Inactive'}
                </span>
              </div>
            </div>
            <button
              className="text-danger ml-3 text-xs"
              onClick={() => setDeleteTarget(wh.id)}
            >
              Delete
            </button>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Delete this webhook? This cannot be undone."
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  )
}
