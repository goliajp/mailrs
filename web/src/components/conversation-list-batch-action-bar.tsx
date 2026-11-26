import type { BatchAction } from '@/hooks/use-mail-mutations'

import { Check, Minus, X } from 'lucide-react'
import { useCallback, useMemo, useRef, useState } from 'react'

import { BatchMoreMenu } from '@/components/conversation-list-batch-more-menu'
import { useBatchKeys } from '@/hooks/use-batch-keys'
import { useElementWidth } from '@/hooks/use-element-width'
import { type Key, useT } from '@/i18n'
import {
  type ActionId,
  ACTIONS,
  batchActions,
  batchExtras,
  batchSelectionOf,
  type RowState,
} from '@/lib/actions'
import { BATCH_ICON_BUTTON } from '@/lib/batch-bar-class'
import { fitCount } from '@/lib/fit-count'

/** An icon button's box and the gap between two, as the classes below draw them. */
const SLOT = { gap: 4, size: 32 }

type Row = { archived: boolean; flagged: boolean; thread_id: string; unread_count: number }

/**
 * The toolbar for the picked conversations, docked under the list: one
 * row at any width. The actions that do not fit go into the menu.
 */
export function BatchActionBar({
  loading,
  onAction,
  onCancel,
  onLabel,
  onToggleAll,
  picked,
  rows,
  view,
}: {
  loading: boolean
  onAction: (action: BatchAction) => void
  onCancel: () => void
  /** Open the label picker for the selection. */
  onLabel: () => void
  /** Pick every loaded row, or none when they all are. */
  onToggleAll: () => void
  picked: ReadonlySet<string>
  rows: Row[]
  /** The Trash offers restoring; every other list, moving there. */
  view: RowState['view']
}) {
  const t = useT()
  const [menuOpen, setMenuOpen] = useState(false)
  const selection = useMemo(() => batchSelectionOf(rows, picked), [rows, picked])
  const actions = useMemo(() => batchActions(view, selection), [view, selection])
  const extras = useMemo(() => batchExtras(selection), [selection])

  const fitRef = useRef<HTMLDivElement>(null)
  const width = useElementWidth(fitRef)
  // before the first layout there is nothing to fit into: all inline
  let shown = actions.length
  if (width !== null) shown = fitCount(width, actions.length, SLOT, extras.length > 0)
  const overflow = [...actions.slice(shown), ...extras]

  const run = useCallback(
    (id: ActionId) => {
      setMenuOpen(false)
      if (id === 'label') {
        onLabel()
        return
      }
      onAction(wireBatchAction(id))
    },
    [onAction, onLabel]
  )
  const escape = useCallback(() => {
    if (menuOpen) setMenuOpen(false)
    else onCancel()
  }, [menuOpen, onCancel])
  // `u` marks unread and `Shift+I` read, whichever of the two is drawn
  const keyed = useMemo(
    () => Array.from(new Set<ActionId>([...actions, 'read', 'unread'])),
    [actions]
  )
  useBatchKeys(keyed, run, escape, loading)

  const allPicked = rows.length > 0 && rows.every((r) => picked.has(r.thread_id))
  return (
    <div
      aria-label={t('batch.toolbar')}
      className="border-border bg-surface absolute inset-x-0 bottom-0 z-40 flex h-12 items-center border-t pr-2 pl-[19px]"
      data-batch-bar
      role="toolbar"
    >
      <SelectAllBox all={allPicked} loading={loading} onClick={onToggleAll} />
      <span className="text-fg ml-3 shrink-0 text-sm font-medium whitespace-nowrap tabular-nums">
        {t('batch.selected', { count: picked.size })}
      </span>
      <div className="ml-2 flex min-w-0 flex-1 items-center gap-1" data-batch-actions ref={fitRef}>
        {actions.slice(0, shown).map((id) => (
          <ActionButton disabled={loading} id={id} key={id} onRun={run} />
        ))}
        {overflow.length > 0 && (
          <BatchMoreMenu
            disabled={loading}
            ids={overflow}
            onOpenChange={setMenuOpen}
            onRun={run}
            open={menuOpen}
          />
        )}
      </div>
      <button
        aria-label={t('list.batchExit')}
        className={`${BATCH_ICON_BUTTON} ml-1`}
        onClick={onCancel}
        title={t('list.batchExit')}
        type="button"
      >
        <X aria-hidden="true" className="h-[18px] w-[18px]" />
      </button>
    </div>
  )
}

function ActionButton({
  disabled,
  id,
  onRun,
}: {
  disabled: boolean
  id: ActionId
  onRun: (id: ActionId) => void
}) {
  const t = useT()
  const spec = ACTIONS[id]
  const Icon = spec.icon
  const label = t(spec.label)
  return (
    <button
      aria-keyshortcuts={spec.keys[0]}
      aria-label={label}
      className={BATCH_ICON_BUTTON}
      data-batch-action={id}
      disabled={disabled}
      onClick={() => onRun(id)}
      title={withKey(t, label, spec.keys[0])}
      type="button"
    >
      <Icon aria-hidden="true" className="h-[18px] w-[18px]" />
    </button>
  )
}

/** The same box as a row's, in the same column: ticked when every loaded row is picked. */
function SelectAllBox({
  all,
  loading,
  onClick,
}: {
  all: boolean
  loading: boolean
  onClick: () => void
}) {
  const t = useT()
  const label = t(selectAllLabel(all))
  if (loading) {
    return (
      <div className="border-border border-t-fg-secondary h-5 w-5 shrink-0 animate-spin rounded-full border-2" />
    )
  }
  return (
    <button
      aria-checked={all ? 'true' : 'mixed'}
      aria-label={label}
      className="touch-target border-accent bg-accent focus-visible:ring-accent/50 flex h-5 w-5 shrink-0 items-center justify-center rounded border-2 focus-visible:ring-2 focus-visible:ring-offset-1 focus-visible:outline-none"
      onClick={onClick}
      role="checkbox"
      title={label}
      type="button"
    >
      {all ? (
        <Check aria-hidden="true" className="text-accent-fg h-3 w-3" />
      ) : (
        <Minus aria-hidden="true" className="text-accent-fg h-3 w-3" />
      )}
    </button>
  )
}

function selectAllLabel(all: boolean): Key {
  if (all) return 'batch.clearAll'
  return 'batch.selectAll'
}

/** The batch route's name for an action: it still calls the Trash `delete`. */
function wireBatchAction(id: ActionId): BatchAction {
  if (id === 'trash') return 'delete'
  return id as BatchAction
}

/** A tooltip that names the key too, when the action has one. */
function withKey(t: ReturnType<typeof useT>, label: string, key: string | undefined) {
  if (!key) return label
  return t('batch.withKey', { key, label })
}
