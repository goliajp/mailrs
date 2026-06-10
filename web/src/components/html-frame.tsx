import DOMPurify from 'dompurify'
import { useEffect, useMemo, useRef } from 'react'

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
    max-width: 680px;
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
export function HtmlFrame({ html, maxHeight }: { html: string; maxHeight?: string }) {
  const hostRef = useRef<HTMLDivElement>(null)
  const sanitized = useMemo(() => cachedSanitize(html), [html])

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
  }, [sanitized])

  return (
    <div
      className={`relative isolate [contain:layout_style_paint] ${maxHeight ? 'overflow-auto' : ''}`}
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
