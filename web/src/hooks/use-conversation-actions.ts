import type { SingleAction } from '@/components/conversation-actions'

import { toast } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { useCallback, useRef, useState } from 'react'

import {
  useArchiveMutation,
  useDeleteMutation,
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
import { ACTIONS } from '@/lib/actions'
import { lastUndoAtom } from '@/store/ui'

// One place that turns an action into the mutation serving it. What the
// action is called, what the toast says and what undoes it are the action
// table's (`lib/actions.ts`); this only knows which mutation runs it.
export type ConversationActions = {
  /** Run an action. One that asks first (`delete`) is held for an answer. */
  act: (threadId: string, action: SingleAction) => Promise<void>
  /** Dismiss the confirmation without deleting. */
  cancelDelete: () => void
  /** Delete the held thread. */
  confirmDelete: () => void
  /** The thread awaiting confirmation, or null. */
  pendingDelete: null | string
}

export function useConversationActions(): ConversationActions {
  const [pendingDelete, setPendingDelete] = useState<null | string>(null)
  const setLastUndo = useSetAtom(lastUndoAtom)
  const mutations = {
    archive: useArchiveMutation(),
    delete: useDeleteMutation(),
    'mark-junk': useMarkJunkMutation(),
    'mark-not-junk': useMarkNotJunkMutation(),
    'mark-notification': useMarkNotificationMutation(),
    'mark-promotion': useMarkPromotionMutation(),
    'move-to-inbox': useMoveToInboxMutation(),
    pin: usePinMutation(),
    read: useMarkReadMutation(),
    snooze: useSnoozeMutation(),
    star: useStarMutation(),
    unarchive: useUnarchiveMutation(),
    unpin: useUnpinMutation(),
    unread: useMarkUnreadMutation(),
    unsnooze: useUnsnoozeMutation(),
    unstar: useUnstarMutation(),
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
      case 'delete':
        m.delete.mutate({ threadId }, opts)
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
    [run, setLastUndo]
  )

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return
    const threadId = pendingDelete
    setPendingDelete(null)
    run(threadId, 'delete', () => toast.success(ACTIONS.delete.done ?? 'Deleted'))
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
