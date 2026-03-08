import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useState } from 'react'
import type { ThemeMode } from '@/lib/theme'

import { postJson } from '@/lib/api'
import { authAtom } from '@/store/auth'
import { notificationsAtom, pageSizeAtom, signatureAtom, signatureEnabledAtom } from '@/store/settings'
import { themeAtom } from '@/store/theme'

const THEME_OPTIONS: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
]

const PAGE_SIZE_OPTIONS = [20, 50, 100, 200]

export function Settings() {
  const auth = useAtomValue(authAtom)
  const setAuth = useSetAtom(authAtom)
  const [theme, setTheme] = useAtom(themeAtom)
  const [pageSize, setPageSize] = useAtom(pageSizeAtom)
  const [notifications, setNotifications] = useAtom(notificationsAtom)
  const [signature, setSignature] = useAtom(signatureAtom)
  const [signatureEnabled, setSignatureEnabled] = useAtom(signatureEnabledAtom)
  const [notificationError, setNotificationError] = useState<string | null>(null)


  const handleLogout = useCallback(async () => {
    try {
      await postJson('/auth/logout', {})
    } catch {
      // ignore
    }
    setAuth(null)
    window.location.href = '/login'
  }, [setAuth])

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
    <div className="flex h-full flex-col overflow-y-auto">
      {/* header */}
      <header className="border-b border-[var(--color-border-default)] px-4 py-3 sm:px-6">
        <h1 className="text-lg font-semibold tracking-tight">Settings</h1>
      </header>

      {/* content */}
      <div className="mx-auto w-full max-w-2xl space-y-8 px-4 py-8 sm:px-6">
        {/* account section */}
        <Section title="Account">
          <div className="space-y-3">
            <Field label="Email">
              <span className="text-sm text-[var(--color-text-secondary)]">{auth?.address ?? '-'}</span>
            </Field>
            <Field label="Display Name">
              <span className="text-sm text-[var(--color-text-secondary)]">{auth?.display_name || '-'}</span>
            </Field>
          </div>
        </Section>

        {/* appearance section */}
        <Section title="Appearance">
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
        </Section>

        {/* conversations section */}
        <Section title="Conversations">
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
        </Section>

        {/* notifications section */}
        <Section title="Notifications">
          <Field label="Browser notifications">
            <div className="flex flex-col gap-1">
              <button
                onClick={() => handleNotificationToggle(!notifications)}
                className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
                  notifications ? 'bg-[var(--color-brand-primary)]' : 'bg-[var(--color-border-strong)]'
                }`}
                role="switch"
                aria-checked={notifications}
              >
                <span
                  className={`inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
                    notifications ? 'translate-x-6' : 'translate-x-1'
                  }`}
                />
              </button>
              {notificationError && (
                <p className="text-xs text-[var(--color-status-danger)]">{notificationError}</p>
              )}
            </div>
          </Field>
        </Section>

        {/* signature section */}
        <Section title="Email Signature">
          <div className="space-y-3">
            <Field label="Enable signature">
              <button
                onClick={() => setSignatureEnabled(!signatureEnabled)}
                className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
                  signatureEnabled ? 'bg-[var(--color-brand-primary)]' : 'bg-[var(--color-border-strong)]'
                }`}
                role="switch"
                aria-checked={signatureEnabled}
              >
                <span
                  className={`inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
                    signatureEnabled ? 'translate-x-6' : 'translate-x-1'
                  }`}
                />
              </button>
            </Field>
            <div>
              <label className="mb-1 block text-sm font-medium text-[var(--color-text-secondary)]">
                Signature text
              </label>
              <textarea
                value={signature}
                onChange={(e) => setSignature(e.target.value)}
                placeholder="Best regards,&#10;Your Name"
                rows={4}
                className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-3 py-2 text-sm text-[var(--color-text-secondary)] placeholder-[var(--color-text-tertiary)] focus:border-[var(--color-brand-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--color-focus-ring)]"
              />
              <p className="mt-1 text-xs text-[var(--color-text-tertiary)]">
                Appended after standard separator (-- ) when sending or replying.
              </p>
            </div>
          </div>
        </Section>

        {/* sign out */}
        <div className="border-t border-[var(--color-border-default)] pt-6">
          <button
            onClick={handleLogout}
            className="rounded-md bg-[var(--color-status-danger)] px-4 py-2 text-sm font-medium text-white transition-colors hover:opacity-90"
          >
            Sign out
          </button>
          {auth?.address && (
            <p className="mt-2 text-xs text-[var(--color-text-tertiary)]">
              Signed in as {auth.address}
            </p>
          )}
        </div>
      </div>
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h2 className="mb-4 text-sm font-semibold uppercase tracking-wider text-[var(--color-text-tertiary)]">
        {title}
      </h2>
      <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4">
        {children}
      </div>
    </section>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-sm font-medium text-[var(--color-text-secondary)]">{label}</span>
      {children}
    </div>
  )
}
