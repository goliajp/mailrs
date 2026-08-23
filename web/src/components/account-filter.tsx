import { useQuery } from '@tanstack/react-query'
import { useAtom } from 'jotai'
import { Check } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { filterLabel, toggledAccounts } from '@/lib/account-filter-rules'
import { settingsKeys } from '@/lib/query-keys'
import { selectedAccountsAtom } from '@/store/ui'
import { wireListExternalAccounts } from '@/wire/endpoints/external-accounts'

/**
 * Narrowing the one list to some of the connected mailboxes.
 *
 * A filter, not a switcher: every box starts ticked and unticking one
 * takes that account out. Somebody with work, personal and two others
 * wants the first two together, which "only this" cannot express.
 *
 * The control hides itself when there is nothing to narrow — with no
 * connected account there is one mailbox and a filter over one thing is
 * furniture.
 */
export function AccountFilter() {
  const accountsQuery = useQuery({
    queryKey: settingsKeys.externalAccounts(),
    staleTime: 60_000,
    queryFn: () => wireListExternalAccounts(),
  })
  const connected = accountsQuery.data ?? []
  const [selected, setSelected] = useAtom(selectedAccountsAtom)
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const away = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', away)
    return () => document.removeEventListener('mousedown', away)
  }, [open])

  if (connected.length === 0) return null

  // The empty id is this deployment's own mail — an account in the
  // list like the rest, so it can be switched off too.
  const rows: { colour: null | string; id: string; label: string }[] = [
    { colour: null, id: '', label: 'This server' },
    ...connected.map((a) => ({
      colour: a.colour ?? null,
      id: a.id,
      label: a.display_name || a.email,
    })),
  ]
  const all = rows.map((r) => r.id)
  const on = selected ?? all
  const narrowed = selected !== null && on.length !== all.length

  // The rule is shared with both phones — a filter that behaves
  // differently on a phone is a filter nobody trusts.
  const toggle = (id: string) => setSelected(toggledAccounts(selected, all, id))

  return (
    <div className="relative" ref={box}>
      <button
        aria-expanded={open}
        aria-haspopup="menu"
        // No `aria-label`: the visible text already names this, and a
        // label that says something else replaces what is on screen
        // for anyone using assistive tech — they would hear "filter by
        // account" and never the "2 of 3" the button is showing.
        className={`rounded-md px-2 py-0.5 text-xs transition-colors ${
          narrowed ? 'bg-fg text-bg' : 'text-fg-secondary hover:bg-bg-secondary'
        }`}
        onClick={() => setOpen((v) => !v)}
        type="button"
      >
        {filterLabel(selected, all)}
      </button>
      {open && (
        <div className="border-border bg-bg absolute z-20 mt-1 w-56 rounded-md border py-1 shadow-lg">
          {rows.map((r) => (
            <button
              className="hover:bg-bg-secondary flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs"
              key={r.id}
              onClick={() => toggle(r.id)}
              type="button"
            >
              <span className="flex h-3 w-3 shrink-0 items-center justify-center">
                {on.includes(r.id) && <Check className="text-accent h-3 w-3" />}
              </span>
              {r.colour !== null && (
                <span
                  aria-hidden
                  className="h-2 w-2 shrink-0 rounded-full"
                  style={{ backgroundColor: r.colour }}
                />
              )}
              <span className="text-fg truncate">{r.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
