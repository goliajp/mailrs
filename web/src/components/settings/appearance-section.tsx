import type { RemoteImages } from '@/store/settings'
import type { ThemeMode } from '@goliapkg/gds'

import { useAtom } from 'jotai'
import { useCallback, useState } from 'react'

import {
  hideSuspectedFraudAtom,
  notificationsAtom,
  notificationSoundAtom,
  remoteImagesAtom,
} from '@/store/settings'
import { themeModeAtom } from '@/store/theme'

import { cardClass, Field, SectionHeader, Toggle } from './_shared'
import { LanguageField } from './language-field'

const REMOTE_IMAGE_OPTIONS: { label: string; value: RemoteImages }[] = [
  { label: 'Show', value: 'auto' },
  { label: 'On click', value: 'click' },
  { label: 'Never', value: 'never' },
]

const THEME_OPTIONS: { label: string; value: ThemeMode }[] = [
  { label: 'Light', value: 'light' },
  { label: 'Dark', value: 'dark' },
  { label: 'System', value: 'system' },
]

export function AppearanceSection() {
  const [theme, setTheme] = useAtom(themeModeAtom)
  const [notifications, setNotifications] = useAtom(notificationsAtom)
  const [notificationSound, setNotificationSound] = useAtom(notificationSoundAtom)
  const [hideSuspectedFraud, setHideSuspectedFraud] = useAtom(hideSuspectedFraudAtom)
  const [remoteImages, setRemoteImages] = useAtom(remoteImagesAtom)
  const [notificationError, setNotificationError] = useState<null | string>(null)

  const handleNotificationToggle = useCallback(
    async (enabled: boolean) => {
      // iOS Safari (non-PWA) has no Notification API at all
      if (enabled && !('Notification' in window)) {
        setNotificationError('Browser notifications are not supported on this device.')
        return
      }
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
        <LanguageField />
      </div>

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

      <div className={cardClass}>
        <div className="space-y-1">
          <Field label="Images in mail">
            <div className="flex gap-1">
              {REMOTE_IMAGE_OPTIONS.map((opt) => (
                <button
                  aria-pressed={remoteImages === opt.value}
                  className={optionClass(remoteImages === opt.value)}
                  key={opt.value}
                  onClick={() => setRemoteImages(opt.value)}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </Field>
          <p className="text-fg-muted text-xs">
            Images never load from the sender&apos;s server: this server fetches a copy when mail
            arrives, which tells the sender only that it was delivered, not whether or when you read
            it. Mail in Junk or held for review never shows images unless you ask.
          </p>
        </div>
      </div>

      <div className={cardClass}>
        <div className="space-y-1">
          <Field label="Hide suspected fraud">
            <Toggle checked={hideSuspectedFraud} onChange={setHideSuspectedFraud} />
          </Field>
          {/* Said plainly, because the alternative reading — that
              turning this off deletes less, or that turning it on
              deletes more — is the one a person would reasonably
              assume. Nothing here throws mail away. */}
          <p className="text-fg-muted text-xs">
            Conversations the receive pipeline judged to be an attempt to defraud you are kept out
            of the ordinary lists. Turning this off shows them in place, marked. Either way they are
            listed under Review, and nothing is deleted.
          </p>
        </div>
      </div>
    </div>
  )
}

function optionClass(selected: boolean): string {
  const base = 'rounded-md px-3 py-1.5 text-sm transition-colors'
  if (selected) return `${base} bg-accent text-accent-fg`
  return `${base} bg-surface text-fg-secondary hover:bg-bg-secondary`
}
