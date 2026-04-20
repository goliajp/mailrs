import DOMPurify from 'dompurify'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { getToken } from '@/store/auth'

const CJK_FONTS =
  "'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji'"

// rewrite external image / link URLs to route through our proxy so we can
// strip trackers and bypass CSP img-src 'self'.
//
// IMPORTANT: do not add loading="lazy" to images here. the email body
// renders inside a sandboxed iframe whose height is measured from
// `doc.body.scrollHeight` after the load event. native lazy-loading
// inside an iframe relies on intersection with the iframe's own
// viewport, which is initially zero — the iframe stays at the
// fallback height (200), the images never enter the "viewport", and
// nothing loads. v1.4.30 added lazy attrs and v1.4.31..v1.4.34 saw
// blank email bodies as a result. decoding="async" is safe; lazy is
// not, in this layout.
// HTML attribute values arrive with entity-encoded specials, e.g.
// LinkedIn signed CDN URLs come through as
//   src="https://media.licdn.com/…?e=…&amp;v=beta&amp;t=…"
// (& is required to be entity-encoded inside attribute values per HTML
// spec). passing that raw string into encodeURIComponent turns the
// '&amp;' into '%26amp%3B', so the upstream sees a literal '&amp;v='
// instead of '&v=', the signature mismatches, and licdn returns 403.
// decode the common entities first so the proxied URL matches the
// original signed URL byte-for-byte.
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

function proxyExternalUrls(html: string): string {
  const token = getToken()
  const tokenParam = token ? `&token=${encodeURIComponent(token)}` : ''
  let result = html.replace(
    /(<img\b)([^>]*\bsrc\s*=\s*["'])(https?:\/\/[^"']+)(["'])/gi,
    (_match, openTag, before, url, after) => {
      const decAttr = /\bdecoding\s*=/i.test(openTag) ? '' : ' decoding="async"'
      const cleanUrl = decodeHtmlEntities(url)
      return `${openTag}${decAttr}${before}/api/proxy/image?url=${encodeURIComponent(cleanUrl)}${tokenParam}${after}`
    }
  )
  result = result.replace(
    /(<a\b[^>]*\bhref\s*=\s*["'])(https?:\/\/[^"']+)(["'])/gi,
    (_match, before, url, after) => {
      const cleanUrl = decodeHtmlEntities(url)
      return `${before}/api/proxy/link?url=${encodeURIComponent(cleanUrl)}${tokenParam}${after}`
    }
  )
  return result
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

// render html email inside a sandboxed iframe for full css isolation.
// CSS containment + isolation on the wrapper guarantee the email's layout
// can never bleed into the surrounding app. wide content is fitted via
// transform: scale() — `zoom` is non-standard and in some Blink layouts can
// disturb sibling/parent metrics.
// when `maxHeight` is set, the wrapper caps the visible area and scrolls
// vertically — useful for previewing a quoted original in the composer
export function HtmlFrame({ html, maxHeight }: { html: string; maxHeight?: string }) {
  const ref = useRef<HTMLIFrameElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(200)
  const [scale, setScale] = useState(1)
  const [containerWidth, setContainerWidth] = useState(0)

  const srcdoc = useMemo(() => {
    const sanitized = sanitizeEmail(html)
    return `<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer">
<style>
  body { margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, 'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Segoe UI', Roboto, 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji', sans-serif; font-size: 14px; line-height: 1.6; color: #1a1a1a; background: #fff; word-wrap: break-word; overflow-wrap: break-word; }
  .mail-wrap { max-width: 680px; margin: 0 auto; padding: 12px; box-sizing: border-box; }
  img { max-width: 100%; height: auto; }
  a { color: #2563eb; }
  pre { overflow-x: auto; }
  blockquote { border-left: 3px solid #d4d4d8; padding-left: 12px; margin: 8px 0; color: #71717a; }
</style>
</head><body><div class="mail-wrap">${sanitized}</div></body></html>`
  }, [html])

  const measure = useCallback(() => {
    const iframe = ref.current
    const container = containerRef.current
    if (!iframe || !container) return
    const doc = iframe.contentDocument
    if (!doc?.body) return

    const contentW = doc.body.scrollWidth
    const containerW = container.clientWidth
    const contentH = doc.body.scrollHeight

    const s = contentW > containerW && containerW > 0 ? containerW / contentW : 1
    setScale(s)
    setContainerWidth(containerW)
    setHeight(contentH * s + 24)
  }, [])

  useEffect(() => {
    const iframe = ref.current
    if (!iframe) return
    const onLoad = () => {
      measure()
      const doc = iframe.contentDocument
      if (doc?.body) {
        const obs = new ResizeObserver(measure)
        obs.observe(doc.body)
        return () => obs.disconnect()
      }
    }
    iframe.addEventListener('load', onLoad)
    return () => iframe.removeEventListener('load', onLoad)
  }, [measure])

  // re-measure on container resize (orientation change)
  useEffect(() => {
    const c = containerRef.current
    if (!c) return
    const obs = new ResizeObserver(measure)
    obs.observe(c)
    return () => obs.disconnect()
  }, [measure])

  // when scaling down, give the iframe its natural width so the content
  // doesn't reflow under the smaller container; the transform shrinks it
  // back into view, and the wrapper's contain locks the box so nothing
  // bleeds out
  const iframeWidth = scale < 1 && containerWidth > 0 ? `${containerWidth / scale}px` : '100%'
  const iframeHeight = scale < 1 ? `${height / scale}px` : `${height}px`

  return (
    <div
      className={`relative isolate [contain:layout_style_paint] ${maxHeight ? 'overflow-auto' : 'overflow-hidden'}`}
      ref={containerRef}
      style={{
        height: maxHeight ? undefined : `${height}px`,
        maxHeight,
      }}
    >
      <iframe
        className="block origin-top-left border-none"
        ref={ref}
        sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
        srcDoc={srcdoc}
        style={{
          height: iframeHeight,
          transform: scale < 1 ? `scale(${scale})` : undefined,
          width: iframeWidth,
        }}
        title="email content"
      />
    </div>
  )
}

function sanitizeEmail(html: string): string {
  const clean = emailPurifier.sanitize(html, {
    ADD_ATTR: ['style', 'align', 'dir', 'bgcolor', 'color', 'face', 'size', 'target', 'rel'],
    ADD_TAGS: ['style'],
    ALLOW_UNKNOWN_PROTOCOLS: false,
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
  })
  return proxyExternalUrls(injectCjkFonts(stripTrackingPixels(clean)))
}
