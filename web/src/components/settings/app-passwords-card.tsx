import type { WireAppPassword } from '@/wire/schemas/app-passwords'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

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
      toast.success('Revoked')
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void refresh()
    }
  }
  return (
    <div className="space-y-3">
      <SectionHeader title="App passwords" />
      <p className="text-fg-muted text-sm">
        Mail apps sign in with an app password, one per device. Use your full address as the user
        name.
      </p>
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
          aria-label="Device name"
          className={inputClass}
          maxLength={100}
          onChange={(e) => setName(e.target.value)}
          placeholder="Device, e.g. iPhone Mail"
          value={name}
        />
        <button className={btnPrimary} disabled={!name.trim()} onClick={create} type="button">
          Create
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
  const { data } = useQuery({
    queryKey: appPasswordsKey,
    queryFn: ({ signal }) => wireListAppPasswords(signal),
  })
  const n = data?.length ?? 0
  if (n === 0) return null
  const revokeAll = async () => {
    try {
      const { revoked } = await wireRevokeAllAppPasswords()
      toast.success(`Revoked ${revoked} app passwords`)
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void queryClient.invalidateQueries({ queryKey: appPasswordsKey })
    }
  }
  return (
    <div className="text-fg-muted flex items-center justify-between gap-3 text-sm">
      <span>
        Changing your password leaves your {n} app passwords working. If someone else may know one,
        revoke them
      </span>
      <button className={btnSecondary} onClick={revokeAll} type="button">
        Revoke all app passwords
      </button>
    </div>
  )
}

function AppPasswordRow({ item, onRevoke }: { item: WireAppPassword; onRevoke: () => void }) {
  let used = 'never used'
  if (item.last_used_at !== null) {
    used = `last used ${new Date(item.last_used_at * 1000).toLocaleString()}`
  }
  return (
    <li className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
      <div className="min-w-0">
        <p className="truncate font-medium">{item.name}</p>
        <p className="text-fg-muted truncate text-xs">
          made {new Date(item.created_at * 1000).toLocaleDateString()} · {used}
        </p>
      </div>
      <button className={btnSecondary} onClick={onRevoke} type="button">
        Revoke
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
  const copy = async () => {
    await navigator.clipboard.writeText(made.password)
    toast.success('Copied')
  }
  return (
    <div className={`${cardClass} space-y-2`} role="status">
      <p className="text-sm">
        App password for <span className="font-medium">{made.name}</span>. It is shown only now
      </p>
      <p className="font-mono text-lg tracking-wider">{grouped(made.password)}</p>
      <div className="flex gap-2">
        <button className={btnSecondary} onClick={copy} type="button">
          Copy
        </button>
        <button className={btnSecondary} onClick={onDone} type="button">
          Done
        </button>
      </div>
    </div>
  )
}
