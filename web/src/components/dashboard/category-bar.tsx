import { cn } from '@/lib/cn'

import { CATEGORY_COLORS, pctToWidth } from './_shared'

export function CategoryBar({
  category,
  count,
  total,
}: {
  category: string
  count: number
  total: number
}) {
  const pct = total > 0 ? Math.round((count / total) * 100) : 0
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs">
        <span className="text-fg-secondary capitalize">{category}</span>
        <span className="text-fg-muted tabular-nums">
          {count} ({pct}%)
        </span>
      </div>
      <div
        aria-label={`${category}: ${pct}%`}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={pct}
        className="bg-bg-secondary h-1.5 overflow-hidden rounded-full"
        role="progressbar"
      >
        <div
          className={cn(
            'h-full rounded-full transition-all',
            CATEGORY_COLORS[category] ?? 'bg-gray-400',
            pctToWidth(pct)
          )}
        />
      </div>
    </div>
  )
}
