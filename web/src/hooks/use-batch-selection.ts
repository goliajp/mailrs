import type { BatchAction } from '@/hooks/use-mail-mutations'

import { toast } from '@goliapkg/gds'
import { useAtom } from 'jotai'
import { useCallback, useState } from 'react'

import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { invalidateBucketMove } from '@/lib/mail-cache'
import { batchModeAtom, selectedThreadIdsAtom } from '@/store/ui'
import { wireBatchMutation } from '@/wire/endpoints/mutations'

/** What takes a batch back, offered in its toast. */
const BATCH_UNDO: Partial<Record<BatchAction, BatchAction>> = {
  archive: 'unarchive',
  delete: 'untrash',
  read: 'unread',
  star: 'unstar',
  unarchive: 'archive',
  unread: 'read',
  unstar: 'star',
  untrash: 'delete',
}

/** The list's batch mode: which conversations are picked, and running an action over them. */
export function useBatchSelection() {
  const t = useT()
  const [batchMode, setBatchMode] = useAtom(batchModeAtom)
  const [picked, setPicked] = useAtom(selectedThreadIdsAtom)
  const [loading, setLoading] = useState(false)

  const exit = useCallback(() => {
    setBatchMode(false)
    setPicked(new Set())
  }, [setBatchMode, setPicked])

  const toggle = useCallback(
    (threadId: string) => {
      setPicked((prev) => {
        const next = new Set(prev)
        if (next.has(threadId)) {
          next.delete(threadId)
        } else {
          next.add(threadId)
        }
        return next
      })
    },
    [setPicked]
  )

  /** Every loaded row, or none when they all already are. */
  const toggleAll = useCallback(
    (threadIds: string[]) => {
      setPicked((prev) => {
        if (threadIds.every((id) => prev.has(id))) return new Set()
        return new Set(threadIds)
      })
    },
    [setPicked]
  )

  const runOn = useCallback(
    async (action: BatchAction, ids: string[], undoing = false) => {
      setLoading(true)
      try {
        const result = await wireBatchMutation(action, ids)
        // How many, not "some": the route reports which ids it could
        // not do, and a message that cannot count is one the reader
        // has to go and check for themselves.
        const refused = result.failed_thread_ids.length || result.failed
        const undo = BATCH_UNDO[action]
        if (!result.success) {
          toast.error(t('batch.partFailed', { failed: refused, total: ids.length }))
        } else if (undo && !undoing) {
          toast.success(t('batch.done', { count: ids.length }), {
            action: { label: t('common.undo'), onClick: () => void runOn(undo, ids, true) },
            duration: 8000,
          })
        } else {
          toast.success(t('batch.done', { count: ids.length }))
        }
        exit()
        // the moved rows leave this list and arrive in another: every
        // list is fetched again, not the next page of this one
        invalidateBucketMove()
      } catch (err) {
        toast.error(errorText(err, t('list.batchFailed')))
      } finally {
        setLoading(false)
      }
    },
    [exit, t]
  )

  // `delete` moves them to the Trash, which keeps them 30 days, so it
  // is not asked
  const run = useCallback(
    async (action: BatchAction) => {
      const ids = Array.from(picked)
      if (ids.length === 0) return
      await runOn(action, ids)
    },
    [picked, runOn]
  )

  return { batchMode, exit, loading, picked, run, setBatchMode, toggle, toggleAll }
}
