import type { WireLabel } from '@/wire/schemas/labels'

import { useLabels } from '@/hooks/use-labels'
import { labelColor } from '@/lib/label-colors'

export function LabelDot({ color }: { color: string }) {
  return (
    <span
      aria-hidden="true"
      className="inline-block h-2 w-2 shrink-0 rounded-full"
      style={{ backgroundColor: labelColor(color) }}
    />
  )
}

/** The labels on a row: up to three, then how many more. */
export function RowLabels({ ids }: { ids: readonly string[] }) {
  const { data } = useLabels()
  if (ids.length === 0 || !data) return null
  const byId = new Map(data.map((l) => [l.id, l]))
  const shown = ids.map((id) => byId.get(id)).filter((l): l is WireLabel => l !== undefined)
  const more = shown.length - 3
  return (
    <span className="flex shrink-0 items-center gap-1">
      {shown.slice(0, 3).map((l) => (
        <span
          className="text-mini max-w-24 truncate rounded px-1.5 leading-4"
          key={l.id}
          style={{ backgroundColor: `${labelColor(l.color)}22`, color: labelColor(l.color) }}
          title={l.name}
        >
          {l.name}
        </span>
      ))}
      {more > 0 && <span className="text-fg-muted text-mini">+{more}</span>}
    </span>
  )
}
