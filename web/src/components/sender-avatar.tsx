import { useEffect, useState } from 'react'

import { avatarColor, avatarInitial } from '@/lib/avatar'
import { cn } from '@/lib/cn'

function extractDomain(sender: string): null | string {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

// extract parent/registrable domain: mail.nvidia.cn → nvidia.cn, em.linkedin.com → linkedin.com
const SECONDARY_TLDS = new Set([
  'ac',
  'co',
  'com',
  'edu',
  'gov',
  'ne',
  'net',
  'or',
  'org',
])
function getParentDomain(domain: string): null | string {
  const parts = domain.split('.')
  if (parts.length <= 2) return null
  // handle multi-part TLDs like .co.jp, .co.uk
  const sld = parts[parts.length - 2]
  if (SECONDARY_TLDS.has(sld) && parts.length >= 3) {
    return parts.length >= 4 ? parts.slice(-3).join('.') : null
  }
  return parts.slice(-2).join('.')
}

// unified icon cache: domain → verified image URL or null
const iconCache = new Map<string, null | string>()
const iconInflight = new Map<string, Promise<null | string>>()

export function SenderAvatar({
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
    size <= 28
      ? 'h-7 w-7 text-[11px]'
      : size <= 32
        ? 'h-8 w-8 text-xs'
        : 'h-9 w-9 text-sm'

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
        className={cn(
          `shrink-0 rounded-full object-cover ${sizeClass}`,
          className
        )}
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
      className={cn(
        `flex shrink-0 items-center justify-center rounded-full font-medium text-white ${sizeClass} ${color}`,
        className
      )}
    >
      {initial}
    </div>
  )
}

// preload an image and verify it's actually a valid image (not an HTML error page)
function probeImage(url: string): Promise<boolean> {
  return new Promise((resolve) => {
    const img = new Image()
    img.onload = () => resolve(img.naturalWidth > 1 && img.naturalHeight > 1)
    img.onerror = () => resolve(false)
    img.src = url
  })
}

// try BIMI first, then apple-touch-icon, cache the winner
function resolveIcon(domain: string): Promise<null | string> {
  if (iconCache.has(domain)) return Promise.resolve(iconCache.get(domain)!)
  const existing = iconInflight.get(domain)
  if (existing) return existing

  const p = (async () => {
    // 1. try BIMI (DNS-backed, always a real SVG)
    try {
      const r = await fetch(`/api/bimi/${domain}`)
      if (r.ok) {
        const data = await r.json()
        if (data?.logo_url) {
          iconCache.set(domain, data.logo_url)
          iconInflight.delete(domain)
          return data.logo_url
        }
      }
    } catch {
      /* continue */
    }

    // 2. try apple-touch-icon: exact domain first, then parent domain
    const domains = [domain]
    const parentDomain = getParentDomain(domain)
    if (parentDomain && parentDomain !== domain) domains.push(parentDomain)

    for (const d of domains) {
      const url = `https://${d}/apple-touch-icon.png`
      if (await probeImage(url)) {
        iconCache.set(domain, url)
        iconInflight.delete(domain)
        return url
      }
    }

    // 3. nothing found
    iconCache.set(domain, null)
    iconInflight.delete(domain)
    return null
  })()

  iconInflight.set(domain, p)
  return p
}
