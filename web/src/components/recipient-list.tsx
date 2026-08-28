import { useEffect, useRef, useState } from 'react'

import { Copyable } from '@/components/copy-button'
import { splitAddresses } from '@/lib/recipients'

/**
 * The `to` / `cc` line, with the addresses still reachable.
 *
 * It used to be `formatRecipients()` — a string of display names with
 * the addresses thrown away, so a line reading `to 29841300, lihao`
 * gave no way to find out who `29841300` is. Which is the question a
 * reader asks precisely when the name looks odd, and this is a mail
 * client where that matters: a display name is the part an impersonator
 * chooses.
 *
 * So each name is a button now, underlined on hover, and pressing one
 * opens a small card with the address it stands for.
 */
export function RecipientList({ label, value }: { label: string; value: string }) {
  const [open, setOpen] = useState<null | number>(null)
  const boxRef = useRef<HTMLSpanElement>(null)

  // Dismissed by a click elsewhere or by Escape. Both, because a
  // popover that only closes on a click strands anyone on a keyboard —
  // the filter panel in this app had exactly that gap.
  useEffect(() => {
    if (open === null) return
    const away = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(null)
    }
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(null)
    }
    document.addEventListener('mousedown', away)
    document.addEventListener('keydown', key)
    return () => {
      document.removeEventListener('mousedown', away)
      document.removeEventListener('keydown', key)
    }
  }, [open])

  const people = splitAddresses(value)
  if (people.length === 0) return null

  return (
    <span className="relative inline-flex min-w-0 items-center gap-1" ref={boxRef}>
      <span className="shrink-0">{label}</span>
      <span className="flex min-w-0 flex-wrap items-center gap-x-1">
        {people.map((person, i) => (
          <span className="inline-flex items-center" key={`${person.address}-${i}`}>
            <button
              className="hover:text-fg focus-visible:ring-accent/50 max-w-[14rem] truncate rounded underline decoration-dotted underline-offset-2 transition-colors focus-visible:ring-2 focus-visible:outline-none"
              onClick={() => setOpen(open === i ? null : i)}
              title={person.address}
              type="button"
            >
              {person.name}
            </button>
            {i < people.length - 1 && <span aria-hidden="true">,</span>}
          </span>
        ))}
      </span>
      {open !== null && people[open] && (
        <span
          className="border-border bg-surface absolute top-full left-0 z-50 mt-1 w-64 rounded-lg border p-3 shadow-lg"
          role="dialog"
        >
          <span className="text-fg block text-sm font-medium">{people[open].name}</span>
          <span className="text-fg-muted mt-1 flex items-center gap-1 text-xs">
            <Copyable value={people[open].address}>
              <span className="min-w-0 truncate select-text">{people[open].address}</span>
            </Copyable>
          </span>
        </span>
      )}
    </span>
  )
}
