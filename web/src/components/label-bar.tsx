import { useAtomValue, useSetAtom } from 'jotai'

import { LabelDot } from '@/components/label-chip'
import { useLabels } from '@/hooks/use-labels'
import { activeLabelAtom, activeListAtom, selectLabelAtom } from '@/store/ui'

/** The user's labels under the tabs; each opens that label's conversations. */
export function LabelBar() {
  const { data } = useLabels()
  const activeList = useAtomValue(activeListAtom)
  const activeLabel = useAtomValue(activeLabelAtom)
  const select = useSetAtom(selectLabelAtom)
  if (!data || data.length === 0) return null
  return (
    <div
      aria-label="Labels"
      className="border-border flex gap-1 overflow-x-auto border-b px-3 py-1.5 text-xs"
      role="toolbar"
    >
      {data.map((l) => (
        <button
          aria-pressed={activeList === 'label' && activeLabel === l.id}
          className={chipClass(activeList === 'label' && activeLabel === l.id)}
          key={l.id}
          onClick={() => select(l.id)}
          type="button"
        >
          <LabelDot color={l.color} />
          <span className="max-w-32 truncate">{l.name}</span>
        </button>
      ))}
    </div>
  )
}

function chipClass(active: boolean): string {
  const base = 'flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 transition-colors'
  if (active) return `${base} bg-bg-tertiary text-fg font-medium`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}
