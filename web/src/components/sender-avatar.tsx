import { cx } from '@goliapkg/gds'
import { memo, useEffect, useState } from 'react'

import { avatarColor, avatarInitial } from '@/lib/avatar'
import { getToken } from '@/store/auth'

function extractDomain(sender: string): null | string {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

// unified icon cache: domain → verified image URL or null
const iconCache = new Map<string, null | string>()
const iconInflight = new Map<string, Promise<null | string>>()

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
  const [iconUrl, setIconUrl] = useState<null | string>(() => {
    if (domain && iconCache.has(domain)) return iconCache.get(domain)!
    return null
  })
  const initial = avatarInitial(sender)
  const color = avatarColor(sender)
  const sizeClass =
    size <= 28 ? 'h-7 w-7 text-[11px]' : size <= 32 ? 'h-8 w-8 text-xs' : 'h-9 w-9 text-sm'

  useEffect(() => {
    if (!domain) return
    if (iconCache.has(domain)) {
      setIconUrl(iconCache.get(domain)!)
      return
    }
    let cancelled = false
    resolveIcon(domain).then((url) => {
      if (!cancelled) setIconUrl(url)
    })
    return () => {
      cancelled = true
    }
  }, [domain])

  // verified icon (BIMI or apple-touch-icon)
  if (iconUrl) {
    return (
      <img
        alt={initial}
        className={cx(`shrink-0 rounded-full object-cover ${sizeClass}`, className)}
        onError={() => {
          iconCache.set(domain!, null)
          setIconUrl(null)
        }}
        src={iconUrl}
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

/**
 * Fetch a small pixmap for `domain` through the mailrs icon cascade
 * (`/api/icon/{domain}` — BIMI → Google favicons → DDG icons). The
 * backend caches the result in kevy so this call is a single kevy
 * hit on a warm cache, not a fanout to external services per render.
 *
 * Wire contract of `/api/icon/{domain}`:
 *   - 200 + image bytes → resolve to a blob URL
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
function resolveIcon(domain: string): Promise<null | string> {
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
          iconCache.set(domain, url)
          iconInflight.delete(domain)
          return url
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
