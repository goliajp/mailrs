import { Mail } from 'lucide-react'

import { cn } from '@/lib/cn'

import { COLOR_MAP } from './_shared'

export function StatCard({
  color,
  icon: Icon,
  label,
  onClick,
  value,
}: {
  color: keyof typeof COLOR_MAP
  icon: typeof Mail
  label: string
  onClick?: () => void
  value: number
}) {
  return (
    <button
      aria-label={`${label}: ${value}`}
      className={cn(
        'border-border flex shrink-0 items-center gap-3 rounded-lg border px-4 py-3 text-left transition-colors md:shrink',
        'w-[140px] md:w-auto',
        onClick ? 'hover:bg-bg-secondary cursor-pointer' : 'cursor-default'
      )}
      onClick={onClick}
      type="button"
    >
      <div className={cn('flex h-9 w-9 items-center justify-center rounded-lg', COLOR_MAP[color])}>
        <Icon aria-hidden="true" className="h-4.5 w-4.5" />
      </div>
      {/* fixed width on the value column so digits widening from 0→N
          doesn't push the card and shift its neighbours */}
      <div className="min-w-[2.5rem]">
        <p className="text-fg text-2xl font-semibold tabular-nums">{value}</p>
        <p className="text-fg-muted text-xs">{label}</p>
      </div>
    </button>
  )
}
