import { toast } from '@goliapkg/gds'
import { useCallback } from 'react'

import {
  useDeleteMutation,
  useMarkReadMutation,
  useMarkUnreadMutation,
  useStarMutation,
  useUnstarMutation,
} from '@/hooks/use-mail-mutations'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'

type ThreadActionDeps = {
  selectedId: null | string
  setIsFlagged: (v: boolean) => void
  setIsRead: (v: boolean) => void
  setSelectedId: (v: null | string) => void
  setShowDeleteConfirm: (v: boolean) => void
}

// the verbs the thread header issues. optimistic local flags are set here
// too, because the header reads them before the mutation settles.
export function useThreadActions({
  selectedId,
  setIsFlagged,
  setIsRead,
  setSelectedId,
  setShowDeleteConfirm,
}: ThreadActionDeps) {
  const refetchThread = useCallback(() => {
    if (!selectedId) return
    queryClient.invalidateQueries({ queryKey: mailKeys.thread(selectedId) }).catch(() => {})
  }, [selectedId])

  const markReadMutation = useMarkReadMutation()
  const markUnreadMutation = useMarkUnreadMutation()
  const starMutation = useStarMutation()
  const unstarMutation = useUnstarMutation()
  const deleteMutation = useDeleteMutation()

  const handleMarkUnread = useCallback(() => {
    if (!selectedId) return
    setIsRead(false)
    markUnreadMutation.mutate(
      { threadId: selectedId },
      {
        onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed'),
        onSuccess: () => toast.success('Marked as unread'),
      }
    )
  }, [selectedId, markUnreadMutation, setIsRead])

  const handleMarkRead = useCallback(() => {
    if (!selectedId) return
    setIsRead(true)
    markReadMutation.mutate(
      { threadId: selectedId },
      {
        onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed'),
        onSuccess: () => toast.success('Marked as read'),
      }
    )
  }, [selectedId, markReadMutation, setIsRead])

  const handleStar = useCallback(() => {
    if (!selectedId) return
    setIsFlagged(true)
    starMutation.mutate(
      { threadId: selectedId },
      { onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed') }
    )
  }, [selectedId, starMutation, setIsFlagged])

  const handleUnstar = useCallback(() => {
    if (!selectedId) return
    setIsFlagged(false)
    unstarMutation.mutate(
      { threadId: selectedId },
      { onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed') }
    )
  }, [selectedId, unstarMutation, setIsFlagged])

  const handleDelete = useCallback(() => {
    if (!selectedId) return
    deleteMutation.mutate(
      { threadId: selectedId },
      {
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : 'Failed')
          setShowDeleteConfirm(false)
        },
        onSuccess: () => {
          toast.success('Deleted')
          setSelectedId(null)
          setShowDeleteConfirm(false)
          // messages come from RQ; invalidation happens via
          // `onSettled` in `useDeleteMutation`, so no local reset here.
        },
      }
    )
  }, [selectedId, deleteMutation, setSelectedId, setShowDeleteConfirm])

  return {
    handleDelete,
    handleMarkRead,
    handleMarkUnread,
    handleStar,
    handleUnstar,
    markReadMutation,
    refetchThread,
  }
}
