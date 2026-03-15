import { useEffect, useState } from 'react'
import { avatarColor, avatarInitial } from '@/lib/avatar'
import { cn } from '@/lib/cn'

function extractDomain(sender: string): string | null {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

// in-memory cache for BIMI lookups: domain → logo URL or null (no record)
const bimiCache = new Map<string, string | null>()

// static logo sources as fallbacks (ordered by quality)
const fallbackSources = [
  (domain: string) => `https://logo.clearbit.com/${domain}`,
  (domain: string) => `https://www.google.com/s2/favicons?domain=${domain}&sz=128`,
]

export function SenderAvatar({ sender, size = 36, className }: {
  sender: string
  size?: number
  className?: string
}) {
  const [bimiUrl, setBimiUrl] = useState<string | null | undefined>(undefined) // undefined = loading
  const [fallbackIndex, setFallbackIndex] = useState(0)
  const domain = extractDomain(sender)
  const initial = avatarInitial(sender)
  const color = avatarColor(sender)
  const sizeClass = size <= 28 ? 'h-7 w-7 text-[11px]' : size <= 32 ? 'h-8 w-8 text-xs' : 'h-9 w-9 text-sm'

  useEffect(() => {
    if (!domain) return
    if (bimiCache.has(domain)) {
      setBimiUrl(bimiCache.get(domain) ?? null)
      return
    }
    let cancelled = false
    fetch(`/api/bimi/${domain}`)
      .then(r => r.ok ? r.json() : null)
      .then(data => {
        if (cancelled) return
        const url = data?.logo_url ?? null
        bimiCache.set(domain, url)
        setBimiUrl(url)
      })
      .catch(() => {
        if (cancelled) return
        bimiCache.set(domain, null)
        setBimiUrl(null)
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
