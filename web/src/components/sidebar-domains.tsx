import { cx } from '@goliapkg/gds'
import { useAtom } from 'jotai'
import { Fragment } from 'react'

import { selectedDomainsAtom } from '@/store/ui'

/**
 * One button per domain the reader can see, under the inbox in the icon
 * rail: reading one domain's mail, or all of it again.
 *
 * The label is the whole domain, allowed to wrap after a dot. It was the
 * first three letters, which read as a cut-off word ("gol") and gave
 * golia.jp and golia.ai the same name.
 */
export function SidebarDomains({ domains, mailOpen }: { domains: string[]; mailOpen: boolean }) {
  const [selected, setSelected] = useAtom(selectedDomainsAtom)
  return domains.map((d) => {
    const active = mailOpen && selected.length === 1 && selected[0] === d
    return (
      <button
        aria-label={d}
        aria-pressed={active}
        className={domainClass(active)}
        key={d}
        onClick={() => setSelected(nextSelection(active, d))}
        title={d}
        type="button"
      >
        {breakAfterDots(d)}
      </button>
    )
  })
}

function breakAfterDots(domain: string) {
  return domain.split('.').map((part, i, parts) => {
    if (i === parts.length - 1) return <Fragment key={i}>{part}</Fragment>
    return (
      <Fragment key={i}>
        {part}.<wbr />
      </Fragment>
    )
  })
}

function domainClass(active: boolean): string {
  const base =
    'w-12 rounded-md px-0.5 py-1 text-center text-[9px] leading-tight font-semibold break-words transition-all duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50'
  if (active) return cx(base, 'bg-accent/10 text-accent')
  return cx(base, 'text-fg-muted hover:bg-bg-tertiary hover:text-fg-secondary')
}

function nextSelection(active: boolean, domain: string): string[] {
  if (active) return []
  return [domain]
}
