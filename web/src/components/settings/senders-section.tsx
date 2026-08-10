import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { queryClient } from '@/lib/query-client'
import {
  type SenderListName,
  wireAddSender,
  wireListSenders,
  wireRemoveSender,
} from '@/wire/endpoints/settings'

import { btnPrimary, btnSecondary, cardClass, inputClass, SectionHeader } from './_shared'

const LISTS: { blurb: string; name: SenderListName; title: string }[] = [
  {
    blurb:
      'Marking a conversation as not junk adds its sender here. Mail from these addresses skips the spam filter.',
    name: 'whitelist',
    title: 'Always allowed',
  },
  {
    blurb: 'Mail from these addresses is treated as junk on arrival.',
    name: 'blacklist',
    title: 'Always blocked',
  },
]

const sendersKey = (list: SenderListName) => ['settings', 'senders', list] as const

/**
 * The senders this account always allows, and always blocks.
 *
 * `spam:{user}:whitelist` is consequential and was invisible: marking
 * a conversation *not junk* adds its sender, the inbound pipeline
 * reads the set on every delivery, and the four routes that show and
 * edit it had no caller on any platform. The list could only grow, and
 * one mistaken tap kept a sender bypassing the filter forever with
 * nothing able to show that it was there.
 */
export function SendersSection() {
  return (
    <div className="space-y-6">
      {LISTS.map((list) => (
        <SenderList key={list.name} {...list} />
      ))}
    </div>
  )
}

function SenderList({
  blurb,
  name,
  title,
}: {
  blurb: string
  name: SenderListName
  title: string
}) {
  const { data: entries = [] } = useQuery({
    queryKey: sendersKey(name),
    queryFn: () => wireListSenders(name).then((rows) => [...rows].sort()),
  })
  const [adding, setAdding] = useState('')
  const [busy, setBusy] = useState(false)

  const invalidate = () => queryClient.invalidateQueries({ queryKey: sendersKey(name) })

  const add = async () => {
    const address = adding.trim()
    if (!address) return
    setBusy(true)
    try {
      await wireAddSender(name, address)
      setAdding('')
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Could not add that address')
    } finally {
      setBusy(false)
    }
  }

  const remove = async (address: string) => {
    try {
      await wireRemoveSender(name, address)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Could not remove that address')
    }
  }

  return (
    <section className={cardClass}>
      <SectionHeader title={title} />
      <p className="text-fg-muted mb-3 text-sm">{blurb}</p>
      <div className="mb-3 flex gap-2">
        <input
          aria-label={`Address to add to ${title}`}
          className={inputClass}
          onChange={(e) => setAdding(e.target.value)}
          placeholder="someone@example.com"
          value={adding}
        />
        <button className={btnPrimary} disabled={busy || !adding.trim()} onClick={() => void add()}>
          Add
        </button>
      </div>
      {entries.length === 0 ? (
        <p className="text-fg-muted text-sm">No addresses.</p>
      ) : (
        <ul className="divide-border divide-y">
          {entries.map((address) => (
            <li className="flex items-center justify-between py-2" key={address}>
              <span className="text-fg text-sm">{address}</span>
              <button className={btnSecondary} onClick={() => void remove(address)}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
