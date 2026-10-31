import type { BatchAction } from '@/hooks/use-mail-mutations'

import { type ActionId, ACTIONS, batchActions, type RowState } from '@/lib/actions'

// floating action bar at bottom of list during batch mode
export function BatchActionBar({
  loading,
  onAction,
  onCancel,
  onLabel,
  selectedCount,
  view,
}: {
  loading: boolean
  onAction: (action: BatchAction) => void
  onCancel: () => void
  /** Open the label picker for the selection. */
  onLabel: () => void
  selectedCount: number
  /** The Trash offers restoring; every other list, moving there. */
  view: RowState['view']
}) {
  return (
    <div className="border-border bg-surface absolute right-0 bottom-0 left-0 z-40 border-t px-3 py-2 backdrop-blur">
      <div className="flex items-center gap-2">
        <span className="text-fg-secondary shrink-0 text-xs font-medium">
          {selectedCount} selected
        </span>
        <div className="flex flex-1 items-center gap-1.5 overflow-x-auto">
          {batchActions(view).map((id) => (
            <button
              className={buttonClass(ACTIONS[id].danger)}
              disabled={loading}
              key={id}
              onClick={() => runBatch(id, onAction, onLabel)}
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

/** The label picker for `label`; the batch route for the rest. */
function runBatch(id: ActionId, onAction: (a: BatchAction) => void, onLabel: () => void) {
  if (id === 'label') {
    onLabel()
    return
  }
  onAction(wireBatchAction(id))
}

/** The batch route's name for an action: it still calls the Trash `delete`. */
function wireBatchAction(id: ActionId): BatchAction {
  if (id === 'trash') return 'delete'
  return id as BatchAction
}
