import type { SingleAction } from '@/components/conversation-actions'

import { toast } from '@goliapkg/gds'
import { useCallback, useState } from 'react'

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

// One place that turns a context-menu verb into the mutation serving it.
// Fifteen mutation hooks and a 170-line switch lived inside
// `ConversationList` until 2026-08-02; nothing in them is specific to that
// component, and the drafts and Send lists want the same verbs.
export type ConversationActions = {
  /** Route a verb to its mutation. `delete` is held for confirmation. */
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
  // single-thread context menu action — each individual mutation runs
  // its own optimistic-update + rollback cycle inside react-query, so this
  // dispatcher only routes by action name. Toast messages remain here so
  // the visual feedback matches the human-facing language.
  const markReadMutation = useMarkReadMutation()
  const markUnreadMutation = useMarkUnreadMutation()
  const starMutation = useStarMutation()
  const unstarMutation = useUnstarMutation()
  const pinMutation = usePinMutation()
  const unpinMutation = useUnpinMutation()
  const archiveMutation = useArchiveMutation()
  const unarchiveMutation = useUnarchiveMutation()
  const snoozeMutation = useSnoozeMutation()
  const unsnoozeMutation = useUnsnoozeMutation()
  const deleteMutation = useDeleteMutation()
  const markJunkMutation = useMarkJunkMutation()
  const markNotJunkMutation = useMarkNotJunkMutation()
  const markNotificationMutation = useMarkNotificationMutation()
  const markPromotionMutation = useMarkPromotionMutation()
  const moveToInboxMutation = useMoveToInboxMutation()
  const act = useCallback(
    async (threadId: string, action: SingleAction) => {
      const onError = (err: unknown) => {
        toast.error(err instanceof Error ? err.message : 'Action failed')
      }
      switch (action) {
        case 'archive':
          archiveMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Archived') }
          )
          break
        case 'delete':
          // Held, not run. Deleting unlinks the maildir files, and the
          // reading pane has always asked first — the list reached the
          // same verb without asking, so a left swipe on a phone was one
          // gesture away from permanent loss. The caller renders
          // `DeleteThreadConfirm` off `pendingDelete`.
          setPendingDelete(threadId)
          break
        case 'mark-junk':
          markJunkMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Moved to Junk') }
          )
          break
        case 'mark-not-junk':
          markNotJunkMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Marked as not junk') }
          )
          break
        case 'mark-notification':
          markNotificationMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Moved to Notifications') }
          )
          break
        case 'mark-promotion':
          markPromotionMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Moved to Promotions') }
          )
          break
        case 'move-to-inbox':
          moveToInboxMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Moved to Inbox') }
          )
          break
        case 'pin':
          pinMutation.mutate({ threadId }, { onError, onSuccess: () => toast.success('Pinned') })
          break
        case 'read':
          markReadMutation.mutate({ threadId }, { onError })
          break
        case 'snooze': {
          const tomorrow = new Date()
          tomorrow.setDate(tomorrow.getDate() + 1)
          tomorrow.setHours(9, 0, 0, 0)
          snoozeMutation.mutate(
            // Epoch seconds, not an ISO string: the handler takes
            // `snoozed_until: i64` and the ISO form 422'd every time.
            { snoozedUntil: Math.floor(tomorrow.getTime() / 1000), threadId },
            { onError, onSuccess: () => toast.success('Snoozed until tomorrow 9:00') }
          )
          break
        }
        case 'star':
          starMutation.mutate({ threadId }, { onError })
          break
        case 'unarchive':
          unarchiveMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Unarchived') }
          )
          break
        case 'unpin':
          unpinMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Unpinned') }
          )
          break
        case 'unread':
          markUnreadMutation.mutate({ threadId }, { onError })
          break
        case 'unsnooze':
          // The way back out. A snooze files the thread away, so
          // without this the only route back is finding it in Archived
          // and unarchiving it, which does not clear the wake time.
          unsnoozeMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Back in your inbox') }
          )
          break
        case 'unstar':
          unstarMutation.mutate({ threadId }, { onError })
          break
      }
    },
    [
      archiveMutation,
      markJunkMutation,
      markNotJunkMutation,
      markNotificationMutation,
      markPromotionMutation,
      markReadMutation,
      markUnreadMutation,
      moveToInboxMutation,
      pinMutation,
      snoozeMutation,
      unsnoozeMutation,
      starMutation,
      unarchiveMutation,
      unpinMutation,
      unstarMutation,
    ]
  )

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return
    const threadId = pendingDelete
    setPendingDelete(null)
    deleteMutation.mutate(
      { threadId },
      {
        onError: (err: unknown) =>
          toast.error(err instanceof Error ? err.message : 'Action failed'),
        onSuccess: () => toast.success('Deleted'),
      }
    )
  }, [pendingDelete, deleteMutation])

  const cancelDelete = useCallback(() => setPendingDelete(null), [])

  return { act, cancelDelete, confirmDelete, pendingDelete }
}
