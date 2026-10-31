import { BottomSheet } from '@/components/bottom-sheet'

/**
 * The question asked before a thread is deleted for good.
 *
 * Deleting moves a thread to the Trash and is not asked; this is the
 * Trash's own delete, which unlinks the maildir files and cannot be
 * undone. `count` is in the question because agreeing to remove forty
 * conversations is a different thing from agreeing to remove one.
 */
export function DeleteThreadConfirm({
  count = 1,
  onCancel,
  onConfirm,
  open,
}: {
  count?: number
  onCancel: () => void
  onConfirm: () => void
  open: boolean
}) {
  if (!open) return null
  return (
    <BottomSheet onClose={onCancel} open>
      <h3 className="text-fg text-sm font-semibold">{deleteTitle(count)}</h3>
      <p className="text-fg-muted mt-1.5 text-sm">
        The messages are deleted and cannot be restored.
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <button
          className="border-border text-fg-secondary hover:bg-bg-secondary rounded-md border px-3 py-2 text-sm transition-colors"
          onClick={onCancel}
          type="button"
        >
          Cancel
        </button>
        <button
          className="bg-danger focus-visible:ring-danger/50 rounded-md px-3 py-2 text-sm font-medium text-white transition-colors hover:opacity-90 focus-visible:ring-2 focus-visible:outline-none"
          onClick={onConfirm}
          type="button"
        >
          Delete forever
        </button>
      </div>
    </BottomSheet>
  )
}

/// The question, with the number in it when there is more than one.
function deleteTitle(count: number): string {
  if (count <= 1) return 'Delete this conversation forever?'
  return `Delete ${count} conversations forever?`
}
