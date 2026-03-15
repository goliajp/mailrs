import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'

import { deleteJson, fetchJson, postJson, putJson } from '@/lib/api'
import type { ThemeMode } from '@/lib/theme'
import { authAtom, getToken } from '@/store/auth'
import { notificationsAtom, pageSizeAtom } from '@/store/settings'
import { themeAtom } from '@/store/theme'

// --- types ---

type Category = 'account' | 'security' | 'signatures' | 'keys' | 'api-keys' | 'appearance'

interface TotpStatus {
  enabled: boolean
}

interface TotpSetup {
  secret: string
  qr_url: string
  recovery_codes: string[]
}

interface Signature {
  id: number
  name: string
  text_content: string
  html_content: string
  is_default: boolean
}

interface KeyStatus {
  pgp_fingerprint: string | null
  smime_fingerprint: string | null
}

interface AgentKey {
  id: string
  name: string
  prefix: string
  expires_at: string | null
  created_at: string
}

interface CreatedAgentKey {
  id: string
  key: string
  prefix: string
}

// --- constants ---

const CATEGORIES: { key: Category; label: string }[] = [
  { key: 'account', label: 'Account' },
  { key: 'security', label: 'Security' },
  { key: 'signatures', label: 'Signatures' },
  { key: 'keys', label: 'Encryption Keys' },
  { key: 'api-keys', label: 'API Keys' },
  { key: 'appearance', label: 'Appearance' },
]

const THEME_OPTIONS: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
]

const PAGE_SIZE_OPTIONS = [20, 50, 100, 200]

const inputClass =
  'w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-base)] px-3 py-1.5 text-sm focus:border-[var(--color-brand-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--color-focus-ring)]'
const btnPrimary =
  'rounded-md bg-[var(--color-bg-inverted)] px-3 py-1.5 text-sm font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90 disabled:opacity-50'
const btnDanger =
  'rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50'
const btnSecondary =
  'rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]'
const cardClass = 'rounded-lg border border-[var(--color-border-default)] p-4'

// --- main component ---

export function Settings() {
  const [active, setActive] = useState<Category>('account')

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <header className="border-b border-[var(--color-border-default)] px-4 py-3 sm:px-6">
        <h1 className="text-lg font-semibold tracking-tight">Settings</h1>
      </header>

      <div className="flex min-h-0 flex-1 flex-col sm:flex-row">
        {/* sidebar - horizontal on mobile, vertical on desktop */}
        <nav className="flex shrink-0 gap-1 overflow-x-auto border-b border-[var(--color-border-default)] p-2 sm:w-48 sm:flex-col sm:overflow-x-visible sm:overflow-y-auto sm:border-b-0 sm:border-r sm:p-3">
          {CATEGORIES.map((cat) => (
            <button
              key={cat.key}
              onClick={() => setActive(cat.key)}
              className={`whitespace-nowrap rounded-md px-3 py-1.5 text-left text-sm transition-colors ${
                active === cat.key
                  ? 'bg-[var(--color-brand-primary)] text-[var(--color-brand-primary-text)]'
                  : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
              }`}
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
  const [pw, setPw] = useState({ current: '', next: '', confirm: '' })
  const [saving, setSaving] = useState(false)

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
      await postJson('/admin/accounts', {
        action: 'change_password',
        current_password: pw.current,
        new_password: pw.next,
      })
      toast.success('Password updated')
      setPw({ current: '', next: '', confirm: '' })
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update password')
    } finally {
      setSaving(false)
    }
  }

  const handleLogout = async () => {
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
            <span className="text-sm text-[var(--color-text-secondary)]">{auth?.address ?? '-'}</span>
          </Field>
          <Field label="Display Name">
            <span className="text-sm text-[var(--color-text-secondary)]">{auth?.display_name || '-'}</span>
          </Field>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-3 text-sm font-medium">Change Password</h3>
        <div className="space-y-2">
          <input
            type="password"
            placeholder="Current password"
            value={pw.current}
            onChange={(e) => setPw({ ...pw, current: e.target.value })}
            className={inputClass}
          />
          <input
            type="password"
            placeholder="New password"
            value={pw.next}
            onChange={(e) => setPw({ ...pw, next: e.target.value })}
            className={inputClass}
          />
          <input
            type="password"
            placeholder="Confirm new password"
            value={pw.confirm}
            onChange={(e) => setPw({ ...pw, confirm: e.target.value })}
            className={inputClass}
          />
          <button onClick={handlePasswordChange} disabled={saving} className={btnPrimary}>
            {saving ? 'Saving...' : 'Update Password'}
          </button>
        </div>
      </div>

      <div className={cardClass}>
        <h3 className="mb-2 text-sm font-medium">Export Mailbox</h3>
        <p className="mb-3 text-xs text-[var(--color-text-tertiary)]">
          Download all your emails as an MBOX file
        </p>
        <button
          onClick={() => window.open(`/api/mail/export?token=${getToken()}`, '_blank')}
          className={btnPrimary}
        >
          Export as MBOX
        </button>
      </div>

      <div className="border-t border-[var(--color-border-default)] pt-4">
        <button onClick={handleLogout} className={btnDanger}>
          Sign out
        </button>
        {auth?.address && (
          <p className="mt-2 text-xs text-[var(--color-text-tertiary)]">Signed in as {auth.address}</p>
        )}
      </div>
    </div>
  )
}

// --- security section ---

function SecuritySection() {
  const [status, setStatus] = useState<TotpStatus | null>(null)
  const [setup, setSetup] = useState<TotpSetup | null>(null)
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

  if (loading) return <p className="text-sm text-[var(--color-text-tertiary)]">Loading...</p>

  return (
    <div className="space-y-6">
      <SectionHeader title="Two-Factor Authentication" />

      <div className={cardClass}>
        <Field label="Status">
          <span
            className={`rounded-full px-2 py-0.5 text-xs font-medium ${
              status?.enabled
                ? 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]'
                : 'bg-[var(--color-bg-raised)] text-[var(--color-text-tertiary)]'
            }`}
          >
            {status?.enabled ? 'Enabled' : 'Disabled'}
          </span>
        </Field>
      </div>

      {!status?.enabled && !setup && (
        <button onClick={handleSetup} className={btnPrimary}>
          Set up 2FA
        </button>
      )}

      {setup && (
        <div className={cardClass + ' space-y-4'}>
          <h3 className="text-sm font-medium">Scan QR Code</h3>
          <div className="rounded-md bg-[var(--color-bg-raised)] p-3">
            <p className="break-all font-mono text-xs text-[var(--color-text-secondary)]">{setup.qr_url}</p>
          </div>

          {setup.recovery_codes.length > 0 && (
            <div className="rounded-lg border border-[var(--color-status-warning)] bg-[var(--color-status-warning-subtle)] p-3">
              <p className="mb-2 text-sm font-semibold">Recovery Codes</p>
              <p className="mb-2 text-xs text-[var(--color-text-secondary)]">
                Save these codes in a safe place. Each code can only be used once.
              </p>
              <div className="grid grid-cols-2 gap-1">
                {setup.recovery_codes.map((rc) => (
                  <code key={rc} className="rounded bg-[var(--color-bg-base)] px-2 py-1 font-mono text-xs">
                    {rc}
                  </code>
                ))}
              </div>
            </div>
          )}

          <div className="flex gap-2">
            <input
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="Enter TOTP code"
              className={inputClass + ' max-w-[200px]'}
            />
            <button onClick={handleEnable} disabled={submitting} className={btnPrimary}>
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
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="Enter TOTP code"
              className={inputClass + ' max-w-[200px]'}
            />
            <button onClick={handleDisable} disabled={submitting} className={btnDanger}>
              {submitting ? 'Disabling...' : 'Disable 2FA'}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

// --- signatures section ---

function SignaturesSection() {
  const [signatures, setSignatures] = useState<Signature[]>([])
  const [editing, setEditing] = useState<Partial<Signature> | null>(null)
  const [saving, setSaving] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<number | null>(null)

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<Signature[]>('/mail/signatures')
      setSignatures(data)
    } catch {
      // keep current
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const handleSave = async () => {
    if (!editing?.name?.trim()) return
    setSaving(true)
    try {
      await postJson('/mail/signatures', {
        id: editing.id,
        name: editing.name.trim(),
        text_content: editing.text_content ?? '',
        html_content: editing.html_content ?? '',
        is_default: editing.is_default ?? false,
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
          onClick={() => setEditing({ name: '', text_content: '', html_content: '', is_default: false })}
          className={btnPrimary}
        >
          Add Signature
        </button>
      </div>

      {editing && (
        <div className={cardClass + ' space-y-3'}>
          <input
            value={editing.name ?? ''}
            onChange={(e) => setEditing({ ...editing, name: e.target.value })}
            placeholder="Signature name"
            className={inputClass}
          />
          <div>
            <label className="mb-1 block text-xs font-medium text-[var(--color-text-secondary)]">Text content</label>
            <textarea
              value={editing.text_content ?? ''}
              onChange={(e) => setEditing({ ...editing, text_content: e.target.value })}
              rows={3}
              className={inputClass}
            />
          </div>
          <div>
            <label className="mb-1 block text-xs font-medium text-[var(--color-text-secondary)]">HTML content</label>
            <textarea
              value={editing.html_content ?? ''}
              onChange={(e) => setEditing({ ...editing, html_content: e.target.value })}
              rows={3}
              className={inputClass}
            />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={editing.is_default ?? false}
              onChange={(e) => setEditing({ ...editing, is_default: e.target.checked })}
            />
            Default signature
          </label>
          <div className="flex gap-2">
            <button onClick={handleSave} disabled={saving} className={btnPrimary}>
              {saving ? 'Saving...' : 'Save'}
            </button>
            <button onClick={() => setEditing(null)} className={btnSecondary}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {signatures.length === 0 && !editing && (
        <p className="text-sm text-[var(--color-text-tertiary)]">No signatures configured</p>
      )}

      {signatures.map((sig) => (
        <div key={sig.id} className={cardClass}>
          <div className="flex items-start justify-between">
            <div>
              <span className="text-sm font-medium">{sig.name}</span>
              {sig.is_default && (
                <span className="ml-2 rounded-full bg-[var(--color-status-success-subtle)] px-2 py-0.5 text-xs text-[var(--color-status-success)]">
                  Default
                </span>
              )}
              {sig.text_content && (
                <p className="mt-1 whitespace-pre-wrap text-xs text-[var(--color-text-tertiary)]">
                  {sig.text_content.slice(0, 120)}
                  {sig.text_content.length > 120 ? '...' : ''}
                </p>
              )}
            </div>
            <div className="flex gap-2">
              <button onClick={() => setEditing(sig)} className="text-xs text-[var(--color-brand-primary)]">
                Edit
              </button>
              <button onClick={() => setDeleteTarget(sig.id)} className="text-xs text-[var(--color-status-danger)]">
                Delete
              </button>
            </div>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Delete this signature? This cannot be undone."
          onConfirm={() => handleDelete(deleteTarget)}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  )
}

// --- encryption keys section ---

function EncryptionKeysSection() {
  const [status, setStatus] = useState<KeyStatus | null>(null)
  const [pgpKey, setPgpKey] = useState('')
  const [smimeCert, setSmimeCert] = useState('')
  const [saving, setSaving] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<KeyStatus>('/mail/keys/status')
      setStatus(data)
    } catch {
      // keep null
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const handleUpload = async (type: 'pgp' | 'smime', content: string) => {
    if (!content.trim()) return
    setSaving(type)
    try {
      await putJson(`/mail/keys/${type}`, { content: content.trim() })
      toast.success(`${type.toUpperCase()} key saved`)
      type === 'pgp' ? setPgpKey('') : setSmimeCert('')
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
      toast.error(e instanceof Error ? e.message : `Failed to delete ${type} key`)
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
          <span className="font-mono text-xs text-[var(--color-text-secondary)]">
            {status?.pgp_fingerprint ?? 'Not configured'}
          </span>
        </Field>
        <textarea
          value={pgpKey}
          onChange={(e) => setPgpKey(e.target.value)}
          placeholder="Paste PGP public key (ASCII-armored)"
          rows={4}
          className={inputClass}
        />
        <div className="flex gap-2">
          <button onClick={() => handleUpload('pgp', pgpKey)} disabled={saving === 'pgp'} className={btnPrimary}>
            {saving === 'pgp' ? 'Saving...' : 'Save PGP Key'}
          </button>
          {status?.pgp_fingerprint && (
            <button onClick={() => handleDelete('pgp')} disabled={saving === 'pgp'} className={btnDanger}>
              Delete
            </button>
          )}
        </div>
      </div>

      <div className={cardClass + ' space-y-4'}>
        <h3 className="text-sm font-medium">S/MIME Certificate</h3>
        <Field label="Fingerprint">
          <span className="font-mono text-xs text-[var(--color-text-secondary)]">
            {status?.smime_fingerprint ?? 'Not configured'}
          </span>
        </Field>
        <textarea
          value={smimeCert}
          onChange={(e) => setSmimeCert(e.target.value)}
          placeholder="Paste S/MIME certificate (PEM format)"
          rows={4}
          className={inputClass}
        />
        <div className="flex gap-2">
          <button onClick={() => handleUpload('smime', smimeCert)} disabled={saving === 'smime'} className={btnPrimary}>
            {saving === 'smime' ? 'Saving...' : 'Save S/MIME Cert'}
          </button>
          {status?.smime_fingerprint && (
            <button onClick={() => handleDelete('smime')} disabled={saving === 'smime'} className={btnDanger}>
              Delete
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

// --- api keys section ---

function ApiKeysSection() {
  const [keys, setKeys] = useState<AgentKey[]>([])
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({ name: '', expires_in_days: '' })
  const [createdKey, setCreatedKey] = useState<CreatedAgentKey | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const data = await fetchJson<AgentKey[]>('/agent/keys')
      setKeys(data)
    } catch {
      // keep current
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const handleCreate = async () => {
    if (!form.name.trim()) return
    try {
      const expires_in_days = form.expires_in_days ? parseInt(form.expires_in_days, 10) : undefined
      const data = await postJson<CreatedAgentKey>('/agent/keys', {
        name: form.name.trim(),
        expires_in_days,
      })
      toast.success('API key created')
      setCreatedKey(data)
      setForm({ name: '', expires_in_days: '' })
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
    navigator.clipboard.writeText(text).then(() => toast.success('Copied to clipboard'))
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title="API Keys" />
        <button onClick={() => setAdding(true)} className={btnPrimary}>
          Create Key
        </button>
      </div>

      {createdKey && (
        <div className="rounded-lg border border-[var(--color-status-warning)] bg-[var(--color-status-warning-subtle)] p-4">
          <p className="mb-2 text-sm font-semibold">API Key Created</p>
          <p className="mb-2 text-xs text-[var(--color-text-secondary)]">
            Copy this key now. It will not be shown again.
          </p>
          <div className="flex items-center gap-2">
            <code className="flex-1 rounded bg-[var(--color-bg-base)] px-3 py-1.5 font-mono text-sm">
              {createdKey.key}
            </code>
            <button onClick={() => copyToClipboard(createdKey.key)} className={btnPrimary}>
              Copy
            </button>
          </div>
          <button
            onClick={() => setCreatedKey(null)}
            className="mt-2 text-xs text-[var(--color-text-secondary)] transition-colors hover:opacity-70"
          >
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <div className={cardClass + ' space-y-3'}>
          <input
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="Key name"
            className={inputClass}
          />
          <input
            value={form.expires_in_days}
            onChange={(e) => setForm({ ...form, expires_in_days: e.target.value })}
            placeholder="Expires in days (optional)"
            type="number"
            min="1"
            className={inputClass}
          />
          <div className="flex gap-2">
            <button onClick={handleCreate} className={btnPrimary}>
              Create
            </button>
            <button onClick={() => setAdding(false)} className={btnSecondary}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {keys.length === 0 && !adding && (
        <p className="text-sm text-[var(--color-text-tertiary)]">No API keys</p>
      )}

      {keys.map((k) => (
        <div key={k.id} className={cardClass}>
          <div className="flex items-center justify-between">
            <div>
              <span className="text-sm font-medium">{k.name}</span>
              <span className="ml-2 font-mono text-xs text-[var(--color-text-tertiary)]">{k.prefix}...</span>
              {k.expires_at && (
                <span className="ml-2 text-xs text-[var(--color-text-tertiary)]">
                  expires {new Date(k.expires_at).toLocaleDateString()}
                </span>
              )}
            </div>
            <button onClick={() => setDeleteTarget(k.id)} className="text-xs text-[var(--color-status-danger)]">
              Revoke
            </button>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Revoke this API key? This cannot be undone."
          onConfirm={() => handleRevoke(deleteTarget)}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  )
}

// --- appearance section ---

function AppearanceSection() {
  const [theme, setTheme] = useAtom(themeAtom)
  const [pageSize, setPageSize] = useAtom(pageSizeAtom)
  const [notifications, setNotifications] = useAtom(notificationsAtom)
  const [notificationError, setNotificationError] = useState<string | null>(null)

  const handleNotificationToggle = useCallback(
    async (enabled: boolean) => {
      if (enabled && Notification.permission === 'default') {
        const result = await Notification.requestPermission()
        if (result === 'denied') {
          setNotificationError('Browser notifications were denied. Please enable them in your browser settings.')
          return
        }
      }
      if (enabled && Notification.permission === 'denied') {
        setNotificationError('Browser notifications are blocked. Please enable them in your browser settings.')
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
                key={opt.value}
                onClick={() => setTheme(opt.value)}
                className={`rounded-md px-3 py-1.5 text-sm transition-colors ${
                  theme === opt.value
                    ? 'bg-[var(--color-brand-primary)] text-[var(--color-brand-primary-text)]'
                    : 'bg-[var(--color-bg-raised)] text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                }`}
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
            value={pageSize}
            onChange={(e) => setPageSize(Number(e.target.value))}
            className="rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-3 py-1.5 text-sm text-[var(--color-text-secondary)] focus:border-[var(--color-brand-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--color-focus-ring)]"
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
        <Field label="Browser notifications">
          <div className="flex flex-col items-end gap-1">
            <Toggle checked={notifications} onChange={(v) => handleNotificationToggle(v)} />
            {notificationError && (
              <p className="text-xs text-[var(--color-status-danger)]">{notificationError}</p>
            )}
          </div>
        </Field>
      </div>
    </div>
  )
}

// --- shared ui components ---

function SectionHeader({ title }: { title: string }) {
  return <h2 className="text-base font-semibold mb-4">{title}</h2>
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-sm font-medium text-[var(--color-text-secondary)]">{label}</span>
      {children}
    </div>
  )
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
        checked ? 'bg-[var(--color-brand-primary)]' : 'bg-[var(--color-border-strong)]'
      }`}
      role="switch"
      aria-checked={checked}
    >
      <span
        className={`inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
          checked ? 'translate-x-6' : 'translate-x-1'
        }`}
      />
    </button>
  )
}

function ConfirmDialog({
  message,
  onConfirm,
  onCancel,
}: {
  message: string
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-sm rounded-lg bg-[var(--color-bg-raised)] p-6" style={{ boxShadow: 'var(--shadow-lg)' }}>
        <p className="mb-4 text-sm text-[var(--color-text-secondary)]">{message}</p>
        <div className="flex justify-end gap-2">
          <button onClick={onCancel} className={btnSecondary}>
            Cancel
          </button>
          <button onClick={onConfirm} className={btnDanger}>
            Confirm
          </button>
        </div>
      </div>
    </div>
  )
}
