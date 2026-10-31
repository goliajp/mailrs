import type { SingleAction } from '@/components/conversation-actions'

import { toast } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { useCallback, useRef, useState } from 'react'

import {
  useArchiveMutation,
  useMarkJunkMutation,
  useMarkNotificationMutation,
  useMarkNotJunkMutation,
  useMarkPromotionMutation,
  useMarkReadMutation,
  useMarkUnreadMutation,
  useMoveToInboxMutation,
  usePinMutation,
  useSnoozeMutation,
  useStarMutation,
  useUnarchiveMutation,
  useUnpinMutation,
  useUnsnoozeMutation,
  useUnstarMutation,
} from '@/hooks/use-mail-mutations'
import {
  useDeleteForeverMutation,
  useTrashMutation,
  useUntrashMutation,
} from '@/hooks/use-trash-mutations'
import { ACTIONS } from '@/lib/actions'
import { labelPickerAtom, lastUndoAtom } from '@/store/ui'

// One place that turns an action into the mutation serving it. What the
// action is called, what the toast says and what undoes it are the action
// table's (`lib/actions.ts`); this only knows which mutation runs it.
export type ConversationActions = {
  /** Run an action. One that asks first (`delete-forever`) is held for an answer. */
  act: (threadId: string, action: SingleAction) => Promise<void>
  /** Dismiss the confirmation without deleting. */
  cancelDelete: () => void
  /** Delete the held thread for good. */
  confirmDelete: () => void
  /** The thread awaiting confirmation, or null. */
  pendingDelete: null | string
}

export function useConversationActions(): ConversationActions {
  const [pendingDelete, setPendingDelete] = useState<null | string>(null)
  const setLastUndo = useSetAtom(lastUndoAtom)
  const openLabels = useSetAtom(labelPickerAtom)
  const mutations = {
    archive: useArchiveMutation(),
    'delete-forever': useDeleteForeverMutation(),
    'mark-junk': useMarkJunkMutation(),
    'mark-not-junk': useMarkNotJunkMutation(),
    'mark-notification': useMarkNotificationMutation(),
    'mark-promotion': useMarkPromotionMutation(),
    'move-to-inbox': useMoveToInboxMutation(),
    pin: usePinMutation(),
    read: useMarkReadMutation(),
    snooze: useSnoozeMutation(),
    star: useStarMutation(),
    trash: useTrashMutation(),
    unarchive: useUnarchiveMutation(),
    unpin: useUnpinMutation(),
    unread: useMarkUnreadMutation(),
    unsnooze: useUnsnoozeMutation(),
    unstar: useUnstarMutation(),
    untrash: useUntrashMutation(),
  }
  const latest = useRef(mutations)
  latest.current = mutations

  const run = useCallback((threadId: string, action: SingleAction, onSuccess: () => void) => {
    const m = latest.current
    const onError = (err: unknown) => {
      toast.error(err instanceof Error ? err.message : 'Action failed')
    }
    const opts = { onError, onSuccess }
    switch (action) {
      // the picker, not a mutation: `act` opens it before reaching here
      case 'label':
        break
      case 'snooze':
        m.snooze.mutate({ snoozedUntil: tomorrowMorning(), threadId }, opts)
        break
      default:
        m[action].mutate({ threadId }, opts)
    }
  }, [])

  const act = useCallback(
    async (threadId: string, action: SingleAction) => {
      const spec = ACTIONS[action]
      if (action === 'label') {
        openLabels({ threadIds: [threadId] })
        return
      }
      if (spec.confirm) {
        setPendingDelete(threadId)
        return
      }
      run(threadId, action, () => {
        const undo = spec.undo
        if (undo) setLastUndo({ action: undo, threadId })
        if (!spec.done) return
        if (!undo) {
          toast.success(spec.done)
          return
        }
        toast.success(spec.done, {
          action: { label: 'Undo', onClick: () => run(threadId, undo, () => {}) },
          duration: 8000,
        })
      })
    },
    [openLabels, run, setLastUndo]
  )

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return
    const threadId = pendingDelete
    setPendingDelete(null)
    run(threadId, 'delete-forever', () =>
      toast.success(ACTIONS['delete-forever'].done ?? 'Deleted forever')
    )
  }, [pendingDelete, run])

  const cancelDelete = useCallback(() => setPendingDelete(null), [])

  return { act, cancelDelete, confirmDelete, pendingDelete }
}

/** Tomorrow at 9:00 local time, in epoch seconds (what the handler takes). */
function tomorrowMorning(): number {
  const t = new Date()
  t.setDate(t.getDate() + 1)
  t.setHours(9, 0, 0, 0)
  return Math.floor(t.getTime() / 1000)
}
