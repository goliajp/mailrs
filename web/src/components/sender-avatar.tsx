import { useState } from 'react'
import { avatarColor, avatarInitial } from '@/lib/avatar'
import { cn } from '@/lib/cn'

function extractDomain(sender: string): string | null {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

// logo sources ordered by quality — try each in sequence
const logoSources = [
  (domain: string) => `https://logo.clearbit.com/${domain}`,
  (domain: string) => `https://www.google.com/s2/favicons?domain=${domain}&sz=128`,
]

export function SenderAvatar({ sender, size = 36, className }: {
  sender: string
  size?: number
  className?: string
}) {
  const [sourceIndex, setSourceIndex] = useState(0)
  const domain = extractDomain(sender)
  const initial = avatarInitial(sender)
  const color = avatarColor(sender)
  const sizeClass = size <= 28 ? 'h-7 w-7 text-[11px]' : size <= 32 ? 'h-8 w-8 text-xs' : 'h-9 w-9 text-sm'

  if (domain && sourceIndex < logoSources.length) {
    return (
      <img
        src={logoSources[sourceIndex](domain)}
        alt={initial}
        onError={() => setSourceIndex(i => i + 1)}
        className={cn(`shrink-0 rounded-full object-cover ${sizeClass}`, className)}
      />
    )
  }

  return (
    <div className={cn(`flex shrink-0 items-center justify-center rounded-full font-medium text-white ${sizeClass} ${color}`, className)}>
      {initial}
    </div>
  )
}
