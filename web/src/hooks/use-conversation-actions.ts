import type { SingleAction } from '@/components/conversation-actions'

import { toast } from '@goliapkg/gds'
import { useCallback } from 'react'

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
  useUnstarMutation,
} from '@/hooks/use-mail-mutations'

// One place that turns a context-menu verb into the mutation serving it.
// Fifteen mutation hooks and a 170-line switch lived inside
// `ConversationList` until 2026-08-02; nothing in them is specific to that
// component, and the drafts and Send lists want the same verbs.
export function useConversationActions() {
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
  const deleteMutation = useDeleteMutation()
  const markJunkMutation = useMarkJunkMutation()
  const markNotJunkMutation = useMarkNotJunkMutation()
  const markNotificationMutation = useMarkNotificationMutation()
  const markPromotionMutation = useMarkPromotionMutation()
  const moveToInboxMutation = useMoveToInboxMutation()
  return useCallback(
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
          deleteMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Deleted') }
          )
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
        case 'unstar':
          unstarMutation.mutate({ threadId }, { onError })
          break
      }
    },
    [
      archiveMutation,
      deleteMutation,
      markJunkMutation,
      markNotJunkMutation,
      markNotificationMutation,
      markPromotionMutation,
      markReadMutation,
      markUnreadMutation,
      moveToInboxMutation,
      pinMutation,
      snoozeMutation,
      starMutation,
      unarchiveMutation,
      unpinMutation,
      unstarMutation,
    ]
  )
}
