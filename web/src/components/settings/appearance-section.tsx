import type { ThemeMode } from '@goliapkg/gds'

import { useAtom } from 'jotai'
import { useCallback, useState } from 'react'

import { notificationsAtom, notificationSoundAtom, pageSizeAtom } from '@/store/settings'
import { themeModeAtom } from '@/store/theme'

import { cardClass, Field, SectionHeader, Toggle } from './_shared'

const THEME_OPTIONS: { label: string; value: ThemeMode }[] = [
  { label: 'Light', value: 'light' },
  { label: 'Dark', value: 'dark' },
  { label: 'System', value: 'system' },
]

const PAGE_SIZE_OPTIONS = [20, 50, 100, 200]

export function AppearanceSection() {
  const [theme, setTheme] = useAtom(themeModeAtom)
  const [pageSize, setPageSize] = useAtom(pageSizeAtom)
  const [notifications, setNotifications] = useAtom(notificationsAtom)
  const [notificationSound, setNotificationSound] = useAtom(notificationSoundAtom)
  const [notificationError, setNotificationError] = useState<null | string>(null)

  const handleNotificationToggle = useCallback(
    async (enabled: boolean) => {
      if (enabled && Notification.permission === 'default') {
        const result = await Notification.requestPermission()
        if (result === 'denied') {
          setNotificationError('Browser notifications were denied.')
          return
        }
      }
      if (enabled && Notification.permission === 'denied') {
        setNotificationError('Browser notifications are blocked.')
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
                aria-pressed={theme === opt.value}
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
            aria-label="Page size"
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
              <Toggle checked={notifications} onChange={(v) => handleNotificationToggle(v)} />
              {notificationError && <p className="text-danger text-xs">{notificationError}</p>}
            </div>
          </Field>
          <Field label="Notification sound">
            <Toggle checked={notificationSound} onChange={setNotificationSound} />
          </Field>
        </div>
      </div>
    </div>
  )
}
