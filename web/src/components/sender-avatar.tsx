import type { IconPlate } from '@/lib/icon-plate'

import { cx } from '@goliapkg/gds'
import { memo, useEffect, useState } from 'react'

import { avatarColor, avatarInitial } from '@/lib/avatar'
import { iconPlate } from '@/lib/icon-plate'
import { getToken } from '@/store/auth'

type Icon = { plate: IconPlate; url: string }

function extractDomain(sender: string): null | string {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

// unified icon cache: domain → verified image and its backing plate, or null
const iconCache = new Map<string, Icon | null>()
const iconInflight = new Map<string, Promise<Icon | null>>()

const PLATE_CLASS: Record<IconPlate, string> = { black: 'bg-black', white: 'bg-white' }

export const SenderAvatar = memo(function SenderAvatar({
  className,
  sender,
  size = 36,
}: {
  className?: string
  sender: string
  size?: number
}) {
  const domain = extractDomain(sender)
  const [icon, setIcon] = useState<Icon | null>(() => {
    if (domain && iconCache.has(domain)) return iconCache.get(domain)!
    return null
  })
  const initial = avatarInitial(sender)
  const color = avatarColor(sender)
  const sizeClass =
    size <= 28 ? 'h-7 w-7 text-mini' : size <= 32 ? 'h-8 w-8 text-xs' : 'h-9 w-9 text-sm'

  useEffect(() => {
    // Clear first, always. This returned early on an empty domain
    // without touching the state, so a reused component kept the
    // previous sender's logo — and the reading pane passes an **empty
    // sender on purpose** for a suspected spoof, precisely so the
    // phish is not drawn with a real brand's avatar. The result was
    // the opposite of the intent: an "iCloud+" phish from
    // `zkxfp.zctxiot.com` rendered with TikTok's logo, borrowed from
    // the message read before it.
    //
    // The same held for any sender whose icon was not yet cached: the
    // old logo stayed on screen until the fetch resolved.
    setIcon(domain ? (iconCache.get(domain) ?? null) : null)
    if (!domain || iconCache.has(domain)) return
    let cancelled = false
    resolveIcon(domain).then((resolved) => {
      if (!cancelled) setIcon(resolved)
    })
    return () => {
      cancelled = true
    }
  }, [domain])

  // verified icon (BIMI or apple-touch-icon)
  if (icon) {
    return (
      // the hairline ring is for icons that bring their own opaque
      // background: it covers the plate, and a black or white disc can
      // match the page exactly
      <img
        alt={initial}
        className={cx(
          `shrink-0 rounded-full object-cover ring-1 ring-black/10 dark:ring-white/25 ${sizeClass} ${PLATE_CLASS[icon.plate]}`,
          className
        )}
        onError={() => {
          iconCache.set(domain!, null)
          setIcon(null)
        }}
        src={icon.url}
      />
    )
  }

  // colored initials — immediate, no blank state
  return (
    <div
      className={cx(
        `flex shrink-0 items-center justify-center rounded-full font-medium text-white ${sizeClass} ${color}`,
        className
      )}
    >
      {initial}
    </div>
  )
})

// a blob: url is same-origin, so the canvas is not tainted and the
// pixels can be read back
async function measurePlate(url: string): Promise<IconPlate> {
  const img = new Image()
  img.src = url
  await img.decode()
  const side = 32
  const canvas = document.createElement('canvas')
  canvas.width = side
  canvas.height = side
  const ctx = canvas.getContext('2d', { willReadFrequently: true })!
  ctx.drawImage(img, 0, 0, side, side)
  return iconPlate(ctx.getImageData(0, 0, side, side).data)
}

/**
 * Fetch a small pixmap for `domain` through the mailrs icon cascade
 * (`/api/icon/{domain}` — BIMI → Google favicons → DDG icons). The
 * backend caches the result in kevy so this call is a single kevy
 * hit on a warm cache, not a fanout to external services per render.
 *
 * Wire contract of `/api/icon/{domain}`:
 *   - 200 + image bytes → resolve to a blob URL and the plate drawn
 *                          behind it (bytes that fail to decode
 *                          resolve to `null`)
 *   - 204 No Content     → resolve to `null` (no icon anywhere;
 *                          fall back to the coloured initial)
 *   - anything else      → resolve to `null` and don't retry within
 *                          the module lifetime
 *
 * The endpoint intentionally uses 204, not 404, so the browser
 * devtools network panel doesn't paint a red row for every unknown
 * sender domain rendered in the inbox — a 401/404 wall was the
 * 2026-07-07 UX regression this replaces.
 */
function resolveIcon(domain: string): Promise<Icon | null> {
  if (iconCache.has(domain)) return Promise.resolve(iconCache.get(domain)!)
  const existing = iconInflight.get(domain)
  if (existing) return existing

  const p = (async () => {
    const token = getToken()
    if (!token) {
      iconCache.set(domain, null)
      iconInflight.delete(domain)
      return null
    }
    try {
      const r = await fetch(`/api/icon/${encodeURIComponent(domain)}`, {
        headers: { Authorization: `Bearer ${token}` },
      })
      if (r.status === 200) {
        const blob = await r.blob()
        if (blob.size > 0) {
          const url = URL.createObjectURL(blob)
          const icon = { plate: await measurePlate(url), url }
          iconCache.set(domain, icon)
          iconInflight.delete(domain)
          return icon
        }
      }
      // 204 or non-2xx → no icon available, cache the null so we
      // don't retry within this page lifetime.
    } catch {
      /* network error: same handling as "not available" */
    }

    iconCache.set(domain, null)
    iconInflight.delete(domain)
    return null
  })()

  iconInflight.set(domain, p)
  return p
}
