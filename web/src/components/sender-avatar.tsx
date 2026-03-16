import { useEffect, useState } from 'react'
import { avatarColor, avatarInitial } from '@/lib/avatar'
import { cn } from '@/lib/cn'

function extractDomain(sender: string): string | null {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

// unified icon cache: domain → verified image URL or null
const iconCache = new Map<string, string | null>()
const iconInflight = new Map<string, Promise<string | null>>()

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
function resolveIcon(domain: string): Promise<string | null> {
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
    } catch { /* continue */ }

    // 2. try apple-touch-icon (preload to verify it's a real image)
    const touchIconUrl = `https://${domain}/apple-touch-icon.png`
    if (await probeImage(touchIconUrl)) {
      iconCache.set(domain, touchIconUrl)
      iconInflight.delete(domain)
      return touchIconUrl
    }

    // 3. nothing found
    iconCache.set(domain, null)
    iconInflight.delete(domain)
    return null
  })()

  iconInflight.set(domain, p)
  return p
}

export function SenderAvatar({ sender, size = 36, className }: {
  sender: string
  size?: number
  className?: string
}) {
  const domain = extractDomain(sender)
  const [iconUrl, setIconUrl] = useState<string | null>(() => {
    if (domain && iconCache.has(domain)) return iconCache.get(domain)!
    return null
  })
  const initial = avatarInitial(sender)
  const color = avatarColor(sender)
  const sizeClass = size <= 28 ? 'h-7 w-7 text-[11px]' : size <= 32 ? 'h-8 w-8 text-xs' : 'h-9 w-9 text-sm'

  useEffect(() => {
    if (!domain) return
    if (iconCache.has(domain)) {
      setIconUrl(iconCache.get(domain)!)
      return
    }
    let cancelled = false
    resolveIcon(domain).then(url => {
      if (!cancelled) setIconUrl(url)
    })
    return () => { cancelled = true }
  }, [domain])

  // verified icon (BIMI or apple-touch-icon)
  if (iconUrl) {
    return (
      <img
        src={iconUrl}
        alt={initial}
        onError={() => {
          iconCache.set(domain!, null)
          setIconUrl(null)
        }}
        className={cn(`shrink-0 rounded-full object-cover ${sizeClass}`, className)}
      />
    )
  }

  // colored initials — immediate, no blank state
  return (
    <div className={cn(`flex shrink-0 items-center justify-center rounded-full font-medium text-white ${sizeClass} ${color}`, className)}>
      {initial}
    </div>
  )
}
