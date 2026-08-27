import { BottomSheet } from '@/components/bottom-sheet'

/**
 * The question asked before a thread is destroyed.
 *
 * Shared because the answer has to be the same wherever the verb is
 * reached from. Deleting a thread unlinks its maildir files — there is
 * no trash and nothing to restore from — and the reading pane has always
 * said so before doing it. The list did not: the row's context menu and
 * a left swipe both went straight to `DELETE`, so on a phone one gesture
 * destroyed mail with a toast for company.
 *
 * `count` because the batch bar reached `DELETE` without asking at all
 * — forty threads unlinked on one click, while deleting one of them
 * from the row beside it was held for an answer. Same verb, same
 * question, and the number is in it: "Delete 40 conversations?" is a
 * different thing to agree to than "Delete conversation?".
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
      <p className="text-fg-muted mt-1.5 text-sm">This will permanently delete all messages.</p>
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
          Delete
        </button>
      </div>
    </BottomSheet>
  )
}

/// The question, with the number in it when there is more than one.
function deleteTitle(count: number): string {
  if (count <= 1) return 'Delete conversation?'
  return `Delete ${count} conversations?`
}
