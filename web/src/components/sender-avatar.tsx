import { useEffect, useState } from 'react'
import { avatarColor, avatarInitial } from '@/lib/avatar'
import { cn } from '@/lib/cn'

function extractDomain(sender: string): string | null {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

// in-memory cache for BIMI lookups: domain → logo URL or null (no record)
const bimiCache = new Map<string, string | null>()

// dedup in-flight requests: domain → promise
const bimiInflight = new Map<string, Promise<string | null>>()

function fetchBimi(domain: string): Promise<string | null> {
  if (bimiCache.has(domain)) return Promise.resolve(bimiCache.get(domain)!)
  const existing = bimiInflight.get(domain)
  if (existing) return existing
  const p = fetch(`/api/bimi/${domain}`)
    .then(r => r.ok ? r.json() : null)
    .then(data => {
      const url = data?.logo_url ?? null
      bimiCache.set(domain, url)
      bimiInflight.delete(domain)
      return url
    })
    .catch(() => {
      bimiCache.set(domain, null)
      bimiInflight.delete(domain)
      return null
    })
  bimiInflight.set(domain, p)
  return p
}

// fallback: apple-touch-icon (180x180 high-res PNG), most major sites have it.
// no google favicon — it returns a blurry grey globe placeholder for missing favicons
// (200 OK, so onError won't fire) which looks worse than colored initials.
const fallbackSources = [
  (domain: string) => `https://${domain}/apple-touch-icon.png`,
]

export function SenderAvatar({ sender, size = 36, className }: {
  sender: string
  size?: number
  className?: string
}) {
  const [bimiUrl, setBimiUrl] = useState<string | null | undefined>(() => {
    const domain = extractDomain(sender)
    if (domain && bimiCache.has(domain)) return bimiCache.get(domain)!
    return undefined
  })
  const [fallbackIndex, setFallbackIndex] = useState(0)
  const domain = extractDomain(sender)
  const initial = avatarInitial(sender)
  const color = avatarColor(sender)
  const sizeClass = size <= 28 ? 'h-7 w-7 text-[11px]' : size <= 32 ? 'h-8 w-8 text-xs' : 'h-9 w-9 text-sm'

  useEffect(() => {
    if (!domain) return
    if (bimiCache.has(domain)) {
      setBimiUrl(bimiCache.get(domain)!)
      return
    }
    let cancelled = false
    fetchBimi(domain).then(url => {
      if (!cancelled) setBimiUrl(url)
    })
    return () => { cancelled = true }
  }, [domain])

  const imgClass = cn(`shrink-0 rounded-full object-cover ${sizeClass}`, className)

  // bimi logo (highest priority)
  if (bimiUrl) {
    return (
      <img
        src={bimiUrl}
        alt={initial}
        onError={() => setBimiUrl(null)}
        className={imgClass}
      />
    )
  }

  // fallback image sources (clearbit → google favicon)
  if (domain && bimiUrl === null && fallbackIndex < fallbackSources.length) {
    return (
      <img
        src={fallbackSources[fallbackIndex](domain)}
        alt={initial}
        onError={() => setFallbackIndex(i => i + 1)}
        className={imgClass}
      />
    )
  }

  // colored initials
  return (
    <div className={cn(`flex shrink-0 items-center justify-center rounded-full font-medium text-white ${sizeClass} ${color}`, className)}>
      {initial}
    </div>
  )
}
