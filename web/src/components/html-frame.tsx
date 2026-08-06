import type { AttachmentInfo } from '@/lib/types'

import DOMPurify from 'dompurify'
import { useEffect, useMemo, useRef } from 'react'

import { fitHeight, fitScale } from '@/lib/fit-to-width'
import { getToken } from '@/store/auth'

const CJK_FONTS =
  "'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji'"

// HTML attribute values arrive with entity-encoded specials, e.g. LinkedIn
// signed CDN URLs come through as src="https://media.licdn.com/…?e=…&amp;v=beta&amp;t=…"
// (& is required to be entity-encoded inside attribute values per HTML spec).
// passing that raw string into encodeURIComponent turns the '&amp;' into
// '%26amp%3B', so the upstream sees a literal '&amp;v=' instead of '&v=', the
// signature mismatches, and licdn returns 403. decode the common entities
// first so the rewritten URL matches the original signed URL byte-for-byte.
function decodeHtmlEntities(s: string): string {
  return s
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    .replace(/&#x27;/gi, "'")
    .replace(/&#39;/gi, "'")
}

// inject CJK fallback fonts into all font-family declarations so kana
// renders correctly on non-Japanese locale systems
function injectCjkFonts(html: string): string {
  return html.replace(/font-family\s*:\s*([^;}"]+)/gi, (match, fonts: string) => {
    if (fonts.includes('Hiragino')) return match
    const trimmed = fonts.trimEnd()
    const endsWithSemiLike = trimmed.endsWith(',')
    const base = endsWithSemiLike ? trimmed.slice(0, -1) : trimmed
    return `font-family: ${base}, ${CJK_FONTS}`
  })
}

// Rewrite external link hrefs to route through /api/proxy/link so click-time
// spam-domain / phishing checks can fire. Image URLs are NOT proxied —
// the Shadow DOM mount sets `referrerpolicy="no-referrer"` on every <img>,
// the browser fetches the external URL directly in parallel (5-10× faster
// than serialising through /api/proxy/image), and `stripTrackingPixels`
// has already deleted 1×1 beacons before this point.
function proxyLinks(html: string): string {
  const token = getToken()
  const tokenParam = token ? `&token=${encodeURIComponent(token)}` : ''
  return html.replace(
    /(<a\b[^>]*\bhref\s*=\s*["'])(https?:\/\/[^"']+)(["'])/gi,
    (_match, before, url, after) => {
      const cleanUrl = decodeHtmlEntities(url)
      return `${before}/api/proxy/link?url=${encodeURIComponent(cleanUrl)}${tokenParam}${after}`
    }
  )
}

// v2.5.0 Phase 5 (RFC-B §5) — MIME `multipart/related` inline images
// are referenced from HTML via `<img src="cid:abc@d.com">` and the
// browser has no idea how to fetch a `cid:` URI. Walk every img tag
// and, when the src matches a known Content-ID, rewrite it to
// `/api/mail/messages/<uid>/attachments/<index>/content`. Any cid:
// with no matching attachment falls through and DOMPurify's default
// `ALLOW_UNKNOWN_PROTOCOLS: false` strips it — the previous behavior.
function rewriteCidImages(html: string, uid: number, attachments: AttachmentInfo[]): string {
  if (!html.includes('cid:')) return html
  // Build the cid → attachment-index map once. cid comparison is
  // case-insensitive per RFC 2392 and reads without angle brackets
  // (the wire type already strips them; strip again defensively).
  const cidToIndex = new Map<string, number>()
  attachments.forEach((att, idx) => {
    if (!att.content_id) return
    const key = att.content_id.replace(/^<|>$/g, '').trim().toLowerCase()
    if (key) cidToIndex.set(key, idx)
  })
  if (cidToIndex.size === 0) return html
  const token = getToken()
  const tokenParam = token ? `?token=${encodeURIComponent(token)}` : ''
  return html.replace(
    /(<img\b[^>]*\bsrc\s*=\s*["'])cid:([^"']+)(["'])/gi,
    (match, before, rawCid, after) => {
      const key = rawCid.replace(/^<|>$/g, '').trim().toLowerCase()
      const idx = cidToIndex.get(key)
      if (idx === undefined) return match
      return `${before}/api/mail/messages/${uid}/attachments/${idx}/content${tokenParam}${after}`
    }
  )
}

// drop common 1×1 tracking-pixel images (open-rate beacons). matches
// the explicit width/height attributes a tracker writes alongside a
// remote-loaded image. defensive only: real content images are never
// authored at width=1 height=1.
function stripTrackingPixels(html: string): string {
  return html.replace(/<img\b[^>]*>/gi, (tag) => {
    const w = /\bwidth\s*=\s*["']?\s*1\s*["']?/i.test(tag)
    const h = /\bheight\s*=\s*["']?\s*1\s*["']?/i.test(tag)
    const inlineSize = /\bstyle\s*=\s*["'][^"']*\b(?:width|height)\s*:\s*1px[^"']*["']/i.test(tag)
    return w && h ? '' : inlineSize && (w || h) ? '' : tag
  })
}

// dedicated DOMPurify instance avoids global hook race conditions in
// concurrent renders
const emailPurifier = DOMPurify()
emailPurifier.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank')
    node.setAttribute('rel', 'noopener noreferrer')
  }
})

// Module-level LRU. DOMPurify + 3 regex transforms run 50-300 ms on
// newsletter-sized bodies; useMemo is component-scoped so unmounting
// (every thread switch) discarded the work. This LRU survives mount/
// unmount — revisiting any of the last MAX_CACHE_ENTRIES emails returns
// the prebuilt body in <1 ms.
const MAX_CACHE_ENTRIES = 50
const sanitizeCache = new Map<string, string>()

// CSS for the Shadow DOM mount. Equivalent to what the old iframe
// srcdoc <style> block had — just scoped to the shadow root instead of
// embedded in a sandboxed document.
const SHADOW_STYLES = `
  :host {
    display: block;
    /* HTML emails are authored against a light background and rarely
       support dark mode. Pin the entire content area to light-mode
       colors regardless of the app theme: the host paints white so a
       narrow .mail-wrap (max-width 680px) doesn't leak the dark app
       background at the sides, and color-scheme keeps form controls /
       scrollbars inside the shadow root rendering light. */
    background: #fff;
    color-scheme: light;
  }
  .mail-wrap {
    /* the app-level GDS reset (* { user-select: none }) hits the shadow
       host, and everything in here computes user-select from it. email
       body text must be selectable, so opt the whole subtree back in —
       document styles can't cross the shadow boundary, so this wins. */
    user-select: text;
    -webkit-user-select: text;
    max-width: 680px;
    /* Fit-to-width scales this element; the origin has to be the top-left
       corner or the shrunk page drifts away from the column it is in.
       The auto margins below centre it, which is right at full size and
       wrong under a transform — the script switches to a left margin
       when it scales. */
    transform-origin: 0 0;
    margin: 0 auto;
    padding: 12px;
    box-sizing: border-box;
    font-family: -apple-system, BlinkMacSystemFont, 'Hiragino Sans',
      'Hiragino Kaku Gothic ProN', 'Segoe UI', Roboto, 'Yu Gothic', 'Meiryo',
      'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji',
      sans-serif;
    font-size: 14px;
    line-height: 1.6;
    color: #1a1a1a;
    background: #fff;
    word-wrap: break-word;
    overflow-wrap: break-word;
  }
  img { max-width: 100%; height: auto; }
  a { color: #2563eb; }
  pre { overflow-x: auto; }
  blockquote {
    border-left: 3px solid #d4d4d8;
    padding-left: 12px;
    margin: 8px 0;
    color: #71717a;
  }
`

// Render html email inside a same-document Shadow DOM for full CSS
// isolation without the iframe round-trip. Replaces the previous
// `<iframe sandbox srcDoc=...>` approach which paid 50-100 ms of paint
// latency on every thread switch, leaked a ResizeObserver per srcDoc
// change, and broke native `loading="lazy"` on images because the
// iframe's nested viewport starts at 200 px and the lazy heuristic
// never fired (see git history for the v1.4.30 blank-email incident).
//
// Shadow DOM gives the same CSS containment as the iframe (rules outside
// don't reach in; rules inside don't escape), but lives in the parent
// document's viewport, so:
//   - native lazy-loading works as the user expects
//   - browsers parallel-fetch external images (5-10× faster than
//     serialising through /api/proxy/image)
//   - no measure() / ResizeObserver round-trip — height is just the
//     content height, free
//   - first paint is one React commit, not commit-iframe-load-measure-
//     commit
//
// External image privacy is preserved by setting
// `referrerpolicy="no-referrer"` on every <img>, so the recipient
// server can't see what user the request originated from.
export function HtmlFrame({
  attachments,
  html,
  maxHeight,
  uid,
}: {
  /**
   * v2.5.0 Phase 5 (RFC-B §5) — attachment list used to rewrite
   * `<img src="cid:...">` references to the runtime attachment
   * URL. Empty array (default) preserves the pre-Phase-5 behavior
   * — the DOMPurify pass strips any surviving cid: URIs since
   * `ALLOW_UNKNOWN_PROTOCOLS` is false.
   */
  attachments?: AttachmentInfo[]
  html: string
  maxHeight?: string
  /** Message uid — needed to construct the attachment URL. */
  uid?: number
}) {
  const hostRef = useRef<HTMLDivElement>(null)
  // Rewrite cid: images BEFORE the sanitize cache lookup so
  // different uid / attachments combinations don't share a cached
  // result (the cache is keyed on the html string).
  const preprocessed = useMemo(() => {
    if (uid === undefined || !attachments || attachments.length === 0) return html
    return rewriteCidImages(html, uid, attachments)
  }, [html, uid, attachments])
  const sanitized = useMemo(() => cachedSanitize(preprocessed), [preprocessed])

  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    const root = host.shadowRoot ?? host.attachShadow({ mode: 'open' })
    root.innerHTML = `<style>${SHADOW_STYLES}</style><div class="mail-wrap">${sanitized}</div>`
    for (const img of root.querySelectorAll<HTMLImageElement>('img')) {
      img.loading = 'lazy'
      img.decoding = 'async'
      img.referrerPolicy = 'no-referrer'
    }

    const wrap = root.querySelector<HTMLElement>('.mail-wrap')
    if (!wrap) return

    // Measured, not guessed: `scrollWidth` is the width the content
    // actually wants, and it is the only thing that knows about a
    // `<table width="700">` nested six levels down. It is a layout value,
    // so a transform does not disturb it.

    // Every property fit() writes, cleared together. Clearing a subset is
    // how a message that scaled once stays wrong after the column grows:
    // an early version left `margin-left: 0` behind, so widening the pane
    // back to full size gave an email that no longer centred.
    const reset = () => {
      wrap.style.transform = ''
      wrap.style.width = ''
      wrap.style.maxWidth = ''
      wrap.style.marginLeft = ''
      host.style.height = ''
    }

    // The column, and the size the content reports in whatever state it
    // is currently in. Stable across passes either way: when scaled, the
    // width is pinned to exactly the content width, so re-reading gives
    // the same number back.
    const state = () => `${host.clientWidth}:${wrap.scrollWidth}:${wrap.scrollHeight}`

    // What the last pass settled on. fit() writes to the elements it is
    // observing, so without a settled state every notification redoes the
    // same arithmetic and rewrites the same values — the browser reports
    // that as `ResizeObserver loop completed with undelivered
    // notifications`, and it is the shape `periodic-work-must-converge`
    // names: idempotent is not the same as convergent.
    let settled = ''

    const fit = () => {
      if (state() === settled) return
      // Measure at full size, or a later pass measures an earlier pass's
      // result and the page shrinks a little further every time.
      reset()
      const hostWidth = host.clientWidth
      const contentWidth = wrap.scrollWidth
      const scale = fitScale(contentWidth, hostWidth)
      if (scale < 1) {
        // Laid out at the content's width, then scaled down to the host's.
        // The stylesheet's `max-width: 680px` has to come off with it —
        // it wins over an inline width, so leaving it on kept the wrap's
        // box (and its white background) narrower than the content
        // painting out of it, a seam down the side of any message whose
        // body carries a background colour.
        wrap.style.width = `${contentWidth}px`
        wrap.style.maxWidth = 'none'
        // The auto margins would centre the *pre-transform* box, pushing
        // the shrunk page off to the right.
        wrap.style.marginLeft = '0'
        wrap.style.transform = `scale(${scale})`
        host.style.height = `${fitHeight(wrap.scrollHeight, scale)}px`
      }
      settled = state()
    }

    fit()
    // Writing inside the observation callback is what makes the browser
    // say the loop went undelivered — the resize it causes lands in the
    // same frame it was told about. A frame's delay puts the write after
    // delivery, and coalesces the two observers firing for one change
    // into a single pass.
    let queued = 0
    const schedule = () => {
      if (queued !== 0) return
      queued = requestAnimationFrame(() => {
        queued = 0
        fit()
      })
    }
    // Images arrive after first paint and change both dimensions, and the
    // column itself changes on rotate and on a pane drag. One observer for
    // both, disconnected on unmount — an earlier iframe version leaked one
    // per body change.
    const ro = new ResizeObserver(schedule)
    ro.observe(host)
    ro.observe(wrap)
    return () => {
      cancelAnimationFrame(queued)
      ro.disconnect()
    }
  }, [sanitized])

  return (
    <div
      // `overflow-x: auto` rather than nothing: a message past the fit
      // floor still has a remainder, and before this it was clipped with
      // no way to reach it — the pixels were simply gone.
      className={`relative isolate overflow-x-auto [contain:layout_style_paint] ${maxHeight ? 'overflow-y-auto' : ''}`}
      ref={hostRef}
      style={{ maxHeight }}
    />
  )
}

function cachedSanitize(html: string): string {
  const hit = sanitizeCache.get(html)
  if (hit !== undefined) {
    // refresh recency: re-insert so LRU eviction skips us
    sanitizeCache.delete(html)
    sanitizeCache.set(html, hit)
    return hit
  }
  const sanitized = sanitizeEmail(html)
  sanitizeCache.set(html, sanitized)
  while (sanitizeCache.size > MAX_CACHE_ENTRIES) {
    const oldest = sanitizeCache.keys().next().value
    if (oldest === undefined) break
    sanitizeCache.delete(oldest)
  }
  return sanitized
}

function sanitizeEmail(html: string): string {
  const clean = emailPurifier.sanitize(html, {
    ADD_ATTR: ['style', 'align', 'dir', 'bgcolor', 'color', 'face', 'size', 'target', 'rel'],
    ADD_TAGS: ['style'],
    ALLOW_UNKNOWN_PROTOCOLS: false,
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
  })
  return proxyLinks(injectCjkFonts(stripTrackingPixels(clean)))
}
