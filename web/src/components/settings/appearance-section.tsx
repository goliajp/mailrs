import type { RemoteImages } from '@/store/settings'
import type { ThemeMode } from '@goliapkg/gds'

import { useAtom } from 'jotai'
import { useCallback, useState } from 'react'

import { type Key, useT } from '@/i18n'
import {
  hideSuspectedFraudAtom,
  notificationsAtom,
  notificationSoundAtom,
  remoteImagesAtom,
} from '@/store/settings'
import { themeModeAtom } from '@/store/theme'

import { cardClass, Field, SectionHeader, Toggle } from './_shared'
import { InboxSectionsCard } from './inbox-sections-card'
import { LanguageField } from './language-field'
import { SwipeCard } from './swipe-card'

const REMOTE_IMAGE_OPTIONS: { label: Key; value: RemoteImages }[] = [
  { label: 'settings.appearance.images.show', value: 'auto' },
  { label: 'settings.appearance.images.click', value: 'click' },
  { label: 'settings.appearance.images.never', value: 'never' },
]

const THEME_OPTIONS: { label: Key; value: ThemeMode }[] = [
  { label: 'settings.appearance.theme.light', value: 'light' },
  { label: 'settings.appearance.theme.dark', value: 'dark' },
  { label: 'settings.appearance.theme.system', value: 'system' },
]

export function AppearanceSection() {
  const t = useT()
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
        setNotificationError(t('settings.appearance.notifyUnsupported'))
        return
      }
      if (enabled && Notification.permission === 'default') {
        const result = await Notification.requestPermission()
        if (result === 'denied') {
          setNotificationError(t('settings.appearance.notifyDenied'))
          return
        }
      }
      if (enabled && Notification.permission === 'denied') {
        setNotificationError(t('settings.appearance.notifyBlocked'))
        return
      }
      setNotificationError(null)
      setNotifications(enabled)
    },
    [setNotifications, t]
  )

  return (
    <div className="space-y-6">
      <SectionHeader title={t('settings.cat.appearance')} />

      <div className={cardClass}>
        <LanguageField />
      </div>

      <div className={cardClass}>
        <Field label={t('settings.appearance.theme')}>
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
                {t(opt.label)}
              </button>
            ))}
          </div>
        </Field>
      </div>

      <div className={cardClass}>
        <div className="space-y-3">
          <Field label={t('settings.appearance.notifications')}>
            <div className="flex flex-col items-end gap-1">
              <Toggle checked={notifications} onChange={(v) => handleNotificationToggle(v)} />
              {notificationError && <p className="text-danger text-xs">{notificationError}</p>}
            </div>
          </Field>
          <Field label={t('settings.appearance.sound')}>
            <Toggle checked={notificationSound} onChange={setNotificationSound} />
          </Field>
        </div>
      </div>

      <div className={cardClass}>
        <div className="space-y-1">
          <Field label={t('settings.appearance.images')}>
            <div className="flex gap-1">
              {REMOTE_IMAGE_OPTIONS.map((opt) => (
                <button
                  aria-pressed={remoteImages === opt.value}
                  className={optionClass(remoteImages === opt.value)}
                  key={opt.value}
                  onClick={() => setRemoteImages(opt.value)}
                >
                  {t(opt.label)}
                </button>
              ))}
            </div>
          </Field>
          <p className="text-fg-muted text-xs">{t('settings.appearance.imagesHelp')}</p>
        </div>
      </div>

      <SwipeCard />

      <InboxSectionsCard />

      <div className={cardClass}>
        <div className="space-y-1">
          <Field label={t('settings.appearance.hideFraud')}>
            <Toggle checked={hideSuspectedFraud} onChange={setHideSuspectedFraud} />
          </Field>
          {/* Said plainly, because the alternative reading — that
              turning this off deletes less, or that turning it on
              deletes more — is the one a person would reasonably
              assume. Nothing here throws mail away. */}
          <p className="text-fg-muted text-xs">{t('settings.appearance.hideFraudHelp')}</p>
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
