import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import {
  currentSubscription,
  disableWebPush,
  enableWebPush,
  webPushSupported,
} from '@/lib/web-push'
import {
  wireImportantContacts,
  wireNotifyOverrides,
  wireNotifySender,
  wireNotifyThread,
  wireSetImportant,
} from '@/wire/endpoints/notify'
import { wireWebPushState } from '@/wire/endpoints/web-push'

import { btnPrimary, btnSecondary, cardClass, inputClass, SectionHeader } from './_shared'
import { WEB_PUSH_KEY } from './notify-shared'

const SUBSCRIBED_KEY = ['settings', 'web-push-here'] as const
const IMPORTANT_KEY = ['settings', 'important'] as const
const OVERRIDES_KEY = ['settings', 'notify-overrides'] as const

/** The contacts whose mail always rings. */
export function ImportantCard() {
  const t = useT()
  const { data: list = [] } = useQuery({ queryFn: wireImportantContacts, queryKey: IMPORTANT_KEY })
  const [adding, setAdding] = useState('')

  const set = async (address: string, important: boolean) => {
    try {
      await wireSetImportant(address, important)
      if (important) setAdding('')
      void queryClient.invalidateQueries({ queryKey: IMPORTANT_KEY })
    } catch (e) {
      toast.error(errorText(e, t('settings.notify.failed')))
    }
  }

  return (
    <section className={cardClass}>
      <SectionHeader title={t('settings.notify.important')} />
      <p className="text-fg-muted mb-3 text-sm">{t('settings.notify.importantBlurb')}</p>
      <div className="mb-3 flex gap-2">
        <input
          aria-label={t('settings.notify.address')}
          className={inputClass}
          onChange={(e) => setAdding(e.target.value)}
          placeholder="someone@example.com"
          value={adding}
        />
        <button
          className={btnPrimary}
          disabled={!adding.includes('@')}
          onClick={() => void set(adding.trim(), true)}
        >
          {t('settings.notify.importantAdd')}
        </button>
      </div>
      {list.length === 0 && (
        <p className="text-fg-muted text-sm">{t('settings.notify.importantNone')}</p>
      )}
      <ul className="divide-border divide-y">
        {list.map((address) => (
          <li className="flex items-center justify-between py-2" key={address}>
            <span className="text-fg text-sm">{address}</span>
            <button className={btnSecondary} onClick={() => void set(address, false)}>
              {t('settings.notify.remove')}
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}

/** Web Push in this browser: off on the server, unsupported here, or on / off. */
export function WebPushCard() {
  const t = useT()
  const { data: server } = useQuery({ queryFn: wireWebPushState, queryKey: WEB_PUSH_KEY })
  const supported = webPushSupported()
  const { data: here = null } = useQuery({
    enabled: supported && server?.enabled === true,
    queryFn: currentSubscription,
    queryKey: SUBSCRIBED_KEY,
  })
  const [busy, setBusy] = useState(false)
  if (!server) return null

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await work()
      void queryClient.invalidateQueries({ queryKey: SUBSCRIBED_KEY })
    } catch (e) {
      toast.error(errorText(e, t('settings.notify.failed')))
    } finally {
      setBusy(false)
    }
  }

  const enable = () =>
    run(async () => {
      const result = await enableWebPush(server.public_key ?? '')
      if (result === 'denied') toast.error(t('settings.notify.webDenied'))
    })

  return (
    <section className={cardClass}>
      <SectionHeader title={t('settings.notify.web')} />
      {!server.enabled && <p className="text-fg-muted text-sm">{t('settings.notify.webOff')}</p>}
      {server.enabled && !supported && (
        <p className="text-fg-muted text-sm">{t('settings.notify.webUnsupported')}</p>
      )}
      {server.enabled && supported && here && (
        <div className="flex items-center justify-between gap-2">
          <span className="text-sm">{t('settings.notify.webOn')}</span>
          <button className={btnSecondary} disabled={busy} onClick={() => void run(disableWebPush)}>
            {t('settings.notify.webDisable')}
          </button>
        </div>
      )}
      {server.enabled && supported && !here && (
        <button className={btnPrimary} disabled={busy} onClick={() => void enable()}>
          {t('settings.notify.webEnable')}
        </button>
      )}
    </section>
  )
}

const CHOICE_LABEL: Record<string, Key> = {
  always: 'settings.notify.always',
  follow: 'settings.notify.follow',
  mute: 'settings.notify.mute',
  never: 'settings.notify.never',
}

/** The per-sender and per-conversation choices, each removable. */
export function OverridesCard() {
  const t = useT()
  const { data } = useQuery({ queryFn: wireNotifyOverrides, queryKey: OVERRIDES_KEY })
  const rows = [
    ...(data?.senders ?? []).map(([field, value]) => ({ field, kind: 'sender' as const, value })),
    ...(data?.threads ?? []).map(([field, value]) => ({ field, kind: 'thread' as const, value })),
  ]

  const clear = async (kind: 'sender' | 'thread', field: string) => {
    try {
      if (kind === 'sender') await wireNotifySender(field, null)
      if (kind === 'thread') await wireNotifyThread(field, null)
      void queryClient.invalidateQueries({ queryKey: OVERRIDES_KEY })
    } catch (e) {
      toast.error(errorText(e, t('settings.notify.failed')))
    }
  }

  return (
    <section className={cardClass}>
      <SectionHeader title={t('settings.notify.overrides')} />
      {rows.length === 0 && (
        <p className="text-fg-muted text-sm">{t('settings.notify.overridesNone')}</p>
      )}
      <ul className="divide-border divide-y">
        {rows.map((r) => (
          <li className="flex items-center justify-between gap-2 py-2" key={`${r.kind}:${r.field}`}>
            <span className="min-w-0 text-sm">
              <span className="text-fg block truncate">{r.field}</span>
              <span className="text-fg-muted block text-xs">{t(CHOICE_LABEL[r.value])}</span>
            </span>
            <button className={btnSecondary} onClick={() => void clear(r.kind, r.field)}>
              {t('settings.notify.remove')}
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}
