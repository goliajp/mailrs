import type { SingleAction } from '@/components/conversation-actions'

import { toast } from '@goliapkg/gds'
import { getDefaultStore, useSetAtom } from 'jotai'
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
import { useReportMutation } from '@/hooks/use-report-mutation'
import {
  useDeleteForeverMutation,
  useTrashMutation,
  useUntrashMutation,
} from '@/hooks/use-trash-mutations'
import { currentLocale, tNow, useT } from '@/i18n'
import { ACTIONS } from '@/lib/actions'
import { errorText } from '@/lib/error-text'
import { offerSenderCorrection } from '@/lib/sender-correction'
import { snoozeSeconds } from '@/lib/snooze-choice'
import { authAtom } from '@/store/auth'
import { labelPickerAtom, lastUndoAtom, snoozePickerAtom } from '@/store/ui'

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
  /** Snooze until `until` (epoch seconds), as picked in the snooze picker. */
  snoozeUntil: (threadId: string, until: number) => void
}

export function useConversationActions(): ConversationActions {
  const t = useT()
  const [pendingDelete, setPendingDelete] = useState<null | string>(null)
  const setLastUndo = useSetAtom(lastUndoAtom)
  const openLabels = useSetAtom(labelPickerAtom)
  const openSnooze = useSetAtom(snoozePickerAtom)
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
    report: useReportMutation(),
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

  // `until` is the snooze time; an undo that snoozes again has none and
  // uses tomorrow morning
  const run = useCallback(
    (threadId: string, action: SingleAction, onSuccess: () => void, until?: number) => {
      const m = latest.current
      const onError = (err: unknown) => {
        toast.error(errorText(err, tNow('action.failed')))
      }
      const opts = { onError, onSuccess }
      switch (action) {
        // the picker, not a mutation: `act` opens it before reaching here
        case 'label':
          break
        case 'snooze':
          m.snooze.mutate(
            { snoozedUntil: until ?? snoozeSeconds('tomorrowMorning', new Date()), threadId },
            opts
          )
          break
        default:
          m[action].mutate({ threadId }, opts)
      }
    },
    []
  )

  // what follows a single action that went through: the sender correction,
  // the undo `z` takes back, and the toast that offers it
  const finish = useCallback(
    (threadId: string, action: SingleAction, done: string) => {
      offerSenderCorrection(threadId, action, getDefaultStore().get(authAtom)?.address ?? '')
      const undo = ACTIONS[action].undo
      if (undo) setLastUndo({ action: undo, threadId })
      if (!undo) {
        toast.success(done)
        return
      }
      toast.success(done, {
        action: { label: t('common.undo'), onClick: () => run(threadId, undo, () => {}) },
        duration: 8000,
      })
    },
    [run, setLastUndo, t]
  )

  const act = useCallback(
    async (threadId: string, action: SingleAction) => {
      const spec = ACTIONS[action]
      if (action === 'label') {
        openLabels({ threadIds: [threadId] })
        return
      }
      // asks until when first; the picker calls `snoozeUntil`
      if (action === 'snooze') {
        openSnooze({ threadId })
        return
      }
      if (spec.confirm) {
        setPendingDelete(threadId)
        return
      }
      run(threadId, action, () => {
        if (!spec.done) {
          offerSenderCorrection(threadId, action, getDefaultStore().get(authAtom)?.address ?? '')
          const undo = spec.undo
          if (undo) setLastUndo({ action: undo, threadId })
          return
        }
        finish(threadId, action, t(spec.done))
      })
    },
    [finish, openLabels, openSnooze, run, setLastUndo, t]
  )

  const snoozeUntil = useCallback(
    (threadId: string, until: number) => {
      const when = new Date(until * 1000).toLocaleString(currentLocale(), {
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
        month: 'short',
        weekday: 'short',
      })
      run(
        threadId,
        'snooze',
        () => finish(threadId, 'snooze', t('action.snooze.done', { when })),
        until
      )
    },
    [finish, run, t]
  )

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return
    const threadId = pendingDelete
    setPendingDelete(null)
    run(threadId, 'delete-forever', () => toast.success(t('action.deleteForever.done')))
  }, [pendingDelete, run, t])

  const cancelDelete = useCallback(() => setPendingDelete(null), [])

  return { act, cancelDelete, confirmDelete, pendingDelete, snoozeUntil }
}
