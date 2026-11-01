import type { WireReason } from '@/wire/schemas/reason'
/**
 * Marks beside the links the server flagged: a link whose text shows one
 * host while it goes to another, or one to a host listed for fraud.
 *
 * The server reads the message as it was stored and names the hosts
 * (`link_warnings` on each message); this finds the matching anchors in
 * the rendered body. A mark shows its reason on hover and on a tap, and
 * a click on the link asks before it opens.
 */
import type { RefObject } from 'react'

import { useAtomValue } from 'jotai'
import { useEffect } from 'react'

import { langAtom, useT } from '@/i18n'
import { reasonText } from '@/lib/reasons'

/** One flagged host, as `link_warnings` on a message carries it. */
export type LinkWarning = {
  readonly host: string
  readonly reasons: readonly WireReason[]
  readonly shown?: string
}

const MARK = 'data-link-warning'

// The body renders in a shadow root the app's stylesheet does not reach,
// so the marks carry their own look.
const MARK_STYLE =
  'display:inline-block;margin-left:2px;padding:0 4px;border-radius:3px;background:#dc2626;color:#fff;font:600 10px/14px sans-serif;cursor:help;vertical-align:1px'
const NOTE_STYLE = 'margin-left:4px;color:#b91c1c;font-size:12px'

/** The host an anchor goes to, read back from the proxied address. */
export function anchorTarget(a: HTMLAnchorElement): null | string {
  const href = a.getAttribute('href') ?? ''
  let url = href
  if (href.startsWith('/api/proxy/link?')) {
    url = new URLSearchParams(href.slice(href.indexOf('?') + 1)).get('url') ?? ''
  }
  try {
    return new URL(url).hostname.toLowerCase()
  } catch {
    return null
  }
}

/**
 * Mark every flagged anchor under `root`. `describe` words a warning's
 * reasons; `confirmOpen` asks the reader and answers whether to open.
 * Returns how many anchors were marked.
 */
export function markLinkWarnings(
  root: ParentNode,
  warnings: readonly LinkWarning[],
  describe: (w: LinkWarning) => string,
  confirmOpen: (text: string) => boolean
): number {
  if (warnings.length === 0) return 0
  let marked = 0
  for (const a of root.querySelectorAll<HTMLAnchorElement>('a[href]')) {
    if (a.hasAttribute(MARK)) continue
    const w = warningFor(a, warnings)
    if (!w) continue
    const text = describe(w)
    a.setAttribute(MARK, '')
    const mark = document.createElement('span')
    mark.className = 'link-warning'
    mark.setAttribute('role', 'img')
    mark.setAttribute('aria-label', text)
    mark.setAttribute('title', text)
    mark.tabIndex = 0
    mark.textContent = '!'
    mark.style.cssText = MARK_STYLE
    const note = document.createElement('span')
    note.className = 'link-warning-note'
    note.textContent = text
    note.hidden = true
    note.style.cssText = NOTE_STYLE
    // a tap shows the reason where a pointer would hover
    mark.addEventListener('click', (e) => {
      e.preventDefault()
      e.stopPropagation()
      note.hidden = !note.hidden
    })
    a.after(mark, note)
    a.addEventListener('click', (e) => {
      if (!confirmOpen(text)) e.preventDefault()
    })
    marked += 1
  }
  return marked
}

/**
 * Mark the flagged links in the body rendered into `host`'s shadow root.
 * Called after the effect that renders it, so it runs on the new body.
 */
export function useLinkWarningMarks(
  host: RefObject<HTMLElement | null>,
  body: string,
  warnings: readonly LinkWarning[] | undefined
): void {
  const t = useT()
  const lang = useAtomValue(langAtom)
  useEffect(() => {
    const root = host.current?.shadowRoot
    if (!root || !warnings) return
    const describe = (w: LinkWarning) =>
      w.reasons.map((r) => reasonText(r, lang, t) ?? r.code).join(' ')
    markLinkWarnings(root, warnings, describe, (text) =>
      window.confirm(t('link.warning.confirm', { reasons: text }))
    )
  }, [host, body, warnings, lang, t])
}

/** The warning `a` falls under, if any. */
export function warningFor(
  a: HTMLAnchorElement,
  warnings: readonly LinkWarning[]
): LinkWarning | undefined {
  const host = anchorTarget(a)
  if (host === null) return undefined
  const text = (a.textContent ?? '').replace(/\s+/g, '').toLowerCase()
  return warnings.find((w) => w.host === host && (!w.shown || text.includes(w.shown)))
}
