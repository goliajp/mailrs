import { useState } from 'react'
import { avatarColor, avatarInitial } from '@/lib/avatar'
import { cn } from '@/lib/cn'

function extractDomain(sender: string): string | null {
  const match = sender.match(/@([a-zA-Z0-9.-]+)/)
  return match ? match[1] : null
}

export function SenderAvatar({ sender, size = 36, className }: {
  sender: string
  size?: number
  className?: string
}) {
  const [imgError, setImgError] = useState(false)
  const domain = extractDomain(sender)
  const initial = avatarInitial(sender)
  const color = avatarColor(sender)
  const sizeClass = size <= 28 ? 'h-7 w-7 text-[11px]' : size <= 32 ? 'h-8 w-8 text-xs' : 'h-9 w-9 text-sm'

  if (domain && !imgError) {
    return (
      <img
        src={`https://www.google.com/s2/favicons?domain=${domain}&sz=${size * 2}`}
        alt={initial}
        onError={() => setImgError(true)}
        className={cn(`shrink-0 rounded-full ${sizeClass}`, className)}
      />
    )
  }

  return (
    <div className={cn(`flex shrink-0 items-center justify-center rounded-full font-medium text-white ${sizeClass} ${color}`, className)}>
      {initial}
    </div>
  )
}
