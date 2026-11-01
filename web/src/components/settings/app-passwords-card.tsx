import type { WireAppPassword } from '@/wire/schemas/app-passwords'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useDate, useDateTime, useT } from '@/i18n'
import { appPasswordsKey, grouped } from '@/lib/app-passwords'
import { queryClient } from '@/lib/query-client'
import {
  wireCreateAppPassword,
  wireListAppPasswords,
  wireRevokeAllAppPasswords,
  wireRevokeAppPassword,
} from '@/wire/endpoints/app-passwords'

import { btnPrimary, btnSecondary, cardClass, inputClass, SectionHeader } from './_shared'
import { errorText } from './rules-shared'

/**
 * Passwords for mail apps (Apple Mail, Outlook, Thunderbird, a phone's
 * mail app). Each is made for one device, shown once, and can be revoked
 * on its own; the account's own password stays for the web.
 */
export function AppPasswordsCard() {
  const t = useT()
  const { data } = useQuery({
    queryKey: appPasswordsKey,
    queryFn: ({ signal }) => wireListAppPasswords(signal),
  })
  const items = data ?? []
  const [name, setName] = useState('')
  const [made, setMade] = useState<null | { name: string; password: string }>(null)
  const refresh = () => queryClient.invalidateQueries({ queryKey: appPasswordsKey })
  const create = async () => {
    try {
      const m = await wireCreateAppPassword(name.trim())
      setMade({ name: m.name, password: m.password })
      setName('')
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void refresh()
    }
  }
  const revoke = async (id: string) => {
    try {
      await wireRevokeAppPassword(id)
      toast.success(t('appPasswords.revoked'))
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void refresh()
    }
  }
  return (
    <div className="space-y-3">
      <SectionHeader title={t('appPasswords.title')} />
      <p className="text-fg-muted text-sm">{t('appPasswords.intro')}</p>
      {made && <MadeNotice made={made} onDone={() => setMade(null)} />}
      {items.length > 0 && (
        <ul className={`${cardClass} divide-border divide-y p-0`}>
          {items.map((a) => (
            <AppPasswordRow item={a} key={a.id} onRevoke={() => revoke(a.id)} />
          ))}
        </ul>
      )}
      <div className="flex gap-2">
        <input
          aria-label={t('appPasswords.deviceName')}
          className={inputClass}
          maxLength={100}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('appPasswords.devicePlaceholder')}
          value={name}
        />
        <button
          className={`${btnPrimary} shrink-0 whitespace-nowrap`}
          disabled={!name.trim()}
          onClick={create}
          type="button"
        >
          {t('appPasswords.create')}
        </button>
      </div>
    </div>
  )
}

/**
 * Shown where the password is changed: changing it leaves app passwords
 * working, so revoking them all is offered beside it.
 */
export function RevokeAllAppPasswords() {
  const t = useT()
  const { data } = useQuery({
    queryKey: appPasswordsKey,
    queryFn: ({ signal }) => wireListAppPasswords(signal),
  })
  const n = data?.length ?? 0
  if (n === 0) return null
  const revokeAll = async () => {
    try {
      const { revoked } = await wireRevokeAllAppPasswords()
      toast.success(t('appPasswords.revokedAll', { n: revoked }))
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void queryClient.invalidateQueries({ queryKey: appPasswordsKey })
    }
  }
  return (
    <div className="text-fg-muted flex items-center justify-between gap-3 text-sm">
      <span>{t('appPasswords.changeNote', { n })}</span>
      <button className={btnSecondary} onClick={revokeAll} type="button">
        {t('appPasswords.revokeAll')}
      </button>
    </div>
  )
}

function AppPasswordRow({ item, onRevoke }: { item: WireAppPassword; onRevoke: () => void }) {
  const t = useT()
  const dateTime = useDateTime()
  const date = useDate()
  let used = t('appPasswords.neverUsed')
  if (item.last_used_at !== null) {
    used = t('appPasswords.lastUsed', { when: dateTime(item.last_used_at) })
  }
  return (
    <li className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
      <div className="min-w-0">
        <p className="truncate font-medium">{item.name}</p>
        <p className="text-fg-muted truncate text-xs">
          {t('appPasswords.made', { when: date(item.created_at) })} · {used}
        </p>
      </div>
      <button className={btnSecondary} onClick={onRevoke} type="button">
        {t('appPasswords.revoke')}
      </button>
    </li>
  )
}

function MadeNotice({
  made,
  onDone,
}: {
  made: { name: string; password: string }
  onDone: () => void
}) {
  const t = useT()
  const copy = async () => {
    await navigator.clipboard.writeText(made.password)
    toast.success(t('common.copied'))
  }
  return (
    <div className={`${cardClass} space-y-2`} role="status">
      <p className="text-sm">{t('appPasswords.shownOnce', { name: made.name })}</p>
      <p className="font-mono text-lg tracking-wider">{grouped(made.password)}</p>
      <div className="flex gap-2">
        <button className={btnSecondary} onClick={copy} type="button">
          {t('common.copy')}
        </button>
        <button className={btnSecondary} onClick={onDone} type="button">
          {t('common.done')}
        </button>
      </div>
    </div>
  )
}
