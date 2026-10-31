import type { BatchAction } from '@/hooks/use-mail-mutations'

import { ACTIONS, BATCH_ACTIONS } from '@/lib/actions'

// floating action bar at bottom of list during batch mode
export function BatchActionBar({
  loading,
  onAction,
  onCancel,
  selectedCount,
}: {
  loading: boolean
  onAction: (action: BatchAction) => void
  onCancel: () => void
  selectedCount: number
}) {
  return (
    <div className="border-border bg-surface absolute right-0 bottom-0 left-0 z-40 border-t px-3 py-2 backdrop-blur">
      <div className="flex items-center gap-2">
        <span className="text-fg-secondary shrink-0 text-xs font-medium">
          {selectedCount} selected
        </span>
        <div className="flex flex-1 items-center gap-1.5 overflow-x-auto">
          {BATCH_ACTIONS.map((id) => (
            <button
              className={buttonClass(ACTIONS[id].danger)}
              disabled={loading}
              key={id}
              onClick={() => onAction(id as BatchAction)}
            >
              {ACTIONS[id].label}
            </button>
          ))}
        </div>
        <button
          className="text-fg-muted hover:bg-bg-secondary shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors disabled:opacity-50"
          disabled={loading}
          onClick={onCancel}
        >
          Cancel
        </button>
        {loading && (
          <div className="border-border border-t-fg-secondary h-4 w-4 shrink-0 animate-spin rounded-full border-2" />
        )}
      </div>
    </div>
  )
}

function buttonClass(danger: boolean): string {
  const base =
    'focus-visible:ring-accent/50 shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50'
  if (danger) return `text-danger hover:bg-danger/10 ${base}`
  return `text-fg-secondary hover:bg-bg-secondary ${base}`
}
