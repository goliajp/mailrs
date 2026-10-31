import { toast } from '@goliapkg/gds'
import { useState } from 'react'

import { BottomSheet } from '@/components/bottom-sheet'
import { useEmptyTrashMutation } from '@/hooks/use-trash-mutations'

/**
 * The line above the Trash: how long it keeps things, and the one way
 * to delete everything in it now. Emptying is asked here and the server
 * may ask for the password again on top, since nothing brings it back.
 */
export function TrashBanner({ empty }: { empty: boolean }) {
  const [asking, setAsking] = useState(false)
  const emptyTrash = useEmptyTrashMutation()
  const confirm = () => {
    setAsking(false)
    emptyTrash.mutate(undefined, {
      onError: (err) => toast.error(errorText(err)),
      onSuccess: (r) => toast.success(deletedLine(r.deleted)),
    })
  }
  return (
    <div className="border-border text-fg-muted flex items-center gap-2 border-b px-3 py-2 text-xs">
      <span className="flex-1">Conversations in the Trash are deleted for good after 30 days</span>
      <button
        className="text-danger hover:bg-danger/10 focus-visible:ring-danger/50 shrink-0 rounded-md px-2.5 py-1 font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50"
        disabled={empty || emptyTrash.isPending}
        onClick={() => setAsking(true)}
        type="button"
      >
        Empty Trash
      </button>
      <BottomSheet onClose={() => setAsking(false)} open={asking}>
        <h3 className="text-fg text-sm font-semibold">Empty the Trash?</h3>
        <p className="text-fg-muted mt-1.5 text-sm">
          Every conversation in it is deleted and cannot be restored.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            className="border-border text-fg-secondary hover:bg-bg-secondary rounded-md border px-3 py-2 text-sm transition-colors"
            onClick={() => setAsking(false)}
            type="button"
          >
            Cancel
          </button>
          <button
            className="bg-danger focus-visible:ring-danger/50 rounded-md px-3 py-2 text-sm font-medium text-white transition-colors hover:opacity-90 focus-visible:ring-2 focus-visible:outline-none"
            onClick={confirm}
            type="button"
          >
            Empty Trash
          </button>
        </div>
      </BottomSheet>
    </div>
  )
}

function deletedLine(n: number): string {
  if (n === 1) return '1 conversation deleted forever'
  return `${n} conversations deleted forever`
}

function errorText(err: unknown): string {
  if (err instanceof Error) return err.message
  return 'Could not empty the Trash'
}
