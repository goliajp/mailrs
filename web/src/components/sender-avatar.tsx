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

// try BIMI logo lookup, cache the result
function resolveIcon(domain: string): Promise<null | string> {
  if (iconCache.has(domain)) return Promise.resolve(iconCache.get(domain)!)
  const existing = iconInflight.get(domain)
  if (existing) return existing

  const p = (async () => {
    // 1. try BIMI (DNS-backed, always a real SVG). The endpoint is
    // Bearer-authed like the rest of the API — an anonymous fetch
    // spams 401s in the console for every distinct sender domain
    // rendered in the inbox. Pass the auth header if we have one, and
    // if we don't (logged-out avatar) skip the network entirely.
    const token = getToken()
    if (!token) {
      iconCache.set(domain, null)
      iconInflight.delete(domain)
      return null
    }
    try {
      const r = await fetch(`/api/bimi/${domain}`, {
        headers: { Authorization: `Bearer ${token}` },
      })
      if (r.ok) {
        const data = await r.json()
        // Backend returns `{ l: "https://…svg", a: "https://…pem" }`
        // (from the raw BIMI TXT record) — the legacy `logo_url` name
        // never existed on the wire. Accept the real key first and
        // keep the legacy key as a fallback for future rewrites.
        const url: string | undefined = data?.l ?? data?.logo_url
        if (url) {
          iconCache.set(domain, url)
          iconInflight.delete(domain)
          return url
        }
      }
    } catch {
      /* continue */
    }

    // no apple-touch-icon fallback — too many 404s and false positives
    // just use letter avatar when BIMI is not available
    iconCache.set(domain, null)
    iconInflight.delete(domain)
    return null
  })()

  iconInflight.set(domain, p)
  return p
}
