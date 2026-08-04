/**
 * The rows of the list on screen, and the current item of it.
 *
 * One place, so that "the first row of Send" means the same thing to the
 * Send list and to the reading pane beside it. Before this, four
 * components each wrote the selection from their own effects and which
 * one won came down to mount order — see `lib/list-selection.ts`.
 *
 * Every source's query is `enabled`-gated on the active list, so the
 * hooks run in a fixed order (React requires that) while only one of
 * them fetches.
 */

import type { SendRow } from '@/components/send-list/send-model'
import type { Draft } from '@/lib/api'
import type { SelectableRow } from '@/lib/list-selection'
import type { ConversationSummary, ThreadMessage } from '@/lib/types'

import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useMemo } from 'react'

import { filterByStatus, joinSends } from '@/components/send-list/send-model'
import { useCurrentMailFilters } from '@/hooks/use-current-mail-filters'
import { useDraftsQuery } from '@/hooks/use-drafts'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import { useThreadQuery } from '@/hooks/use-mail-queries'
import { useSendsQuery } from '@/hooks/use-sends'
import { useSentMessagesQuery } from '@/hooks/use-sent-messages'
import { narrowConversations } from '@/lib/list-rows'
import { resolveSelection } from '@/lib/list-selection'
import { MAIL_LISTS } from '@/lib/mail-lists'
import {
  activeListAtom,
  draftQueryAtom,
  importanceSectionAtom,
  pickedItemAtom,
  pickInListAtom,
  quickFilterAtom,
  selectedDomainsAtom,
  sendQueryAtom,
  sendStatusFilterAtom,
  sortOrderAtom,
  stickyUnreadIdsAtom,
} from '@/store/ui'

const EMPTY_MESSAGES: readonly ThreadMessage[] = []
const NO_ROWS: readonly SelectableRow[] = []

/** The conversation rows the list draws, in the order it draws them. */
export function useConversationRows(): {
  hasMore: boolean
  initialLoading: boolean
  loadingMore: boolean
  loadMore: () => Promise<void>
  refresh: () => Promise<void>
  rows: ConversationSummary[]
} {
  const filters = useCurrentMailFilters()
  const quickFilter = useAtomValue(quickFilterAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const sortOrder = useAtomValue(sortOrderAtom)
  const stickyUnread = useAtomValue(stickyUnreadIdsAtom)
  const { conversations, hasMore, initialLoading, loadingMore, loadMore, refresh } =
    useFlatConversations(filters)

  const rows = useMemo(
    () =>
      narrowConversations(conversations, {
        importanceSection,
        quickFilter,
        sortOrder,
        stickyUnread,
      }),
    [conversations, importanceSection, quickFilter, sortOrder, stickyUnread]
  )
  return { hasMore, initialLoading, loadingMore, loadMore, refresh, rows }
}

/**
 * The rows of whichever list is showing, reduced to what the reading
 * pane needs.
 *
 * Draft is empty on purpose and not for want of rows: a draft opens the
 * composer, so auto-selecting one would pop it open the moment you
 * arrived at the tab.
 */
export function useCurrentListRows(): readonly SelectableRow[] {
  const list = useAtomValue(activeListAtom)
  const conversations = useConversationRows()
  const sends = useSendRows()
  // Called for hook-order stability; its rows are deliberately not
  // selectable, so nothing here reads them.
  useDraftRows()

  return useMemo(() => {
    switch (MAIL_LISTS[list].source.kind) {
      case 'drafts':
        return NO_ROWS
      case 'sends':
        return sends.rows.map((r) => ({ threadId: r.threadId, uid: r.uid }))
      case 'threads':
        return conversations.rows.map((c) => ({ threadId: c.thread_id, uid: null }))
    }
  }, [list, conversations.rows, sends.rows])
}

/** The current item of the current list. */
export function useCurrentSelection(): null | SelectableRow {
  const list = useAtomValue(activeListAtom)
  const picked = useAtomValue(pickedItemAtom)
  const rows = useCurrentListRows()
  return useMemo(() => resolveSelection(list, picked, rows), [list, picked, rows])
}

/** The messages of that thread. */
export function useCurrentThreadMessages(): readonly ThreadMessage[] {
  const selectedThreadId = useSelectedThreadId()
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const { data } = useThreadQuery(selectedThreadId, selectedDomains)
  return data ?? EMPTY_MESSAGES
}

/** The Draft rows the list draws, after its search narrowing. */
export function useDraftRows(): { all: Draft[]; loading: boolean; rows: Draft[] } {
  const enabled = useAtomValue(activeListAtom) === 'draft'
  const query = useAtomValue(draftQueryAtom)
  const { data, isLoading } = useDraftsQuery(enabled)
  const all = useMemo(() => data ?? [], [data])

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return all
    return all.filter(
      (d) =>
        d.subject.toLowerCase().includes(q) ||
        d.to.toLowerCase().includes(q) ||
        d.body.toLowerCase().includes(q)
    )
  }, [all, query])

  return { all, loading: isLoading, rows }
}

/** The thread the reading pane shows. */
export function useSelectedThreadId(): null | string {
  return useCurrentSelection()?.threadId ?? null
}

/**
 * Move the selection by thread id, for the callers that navigate rather
 * than click a row — prev/next, and the delete that has to leave the
 * thread it just removed.
 *
 * `null` clears the pick, which is not "nothing is selected": the list
 * falls back to its first row. That is what a delete wants, and it is
 * why nothing has to compute the neighbour by hand any more.
 */
export function useSelectThreadId(): (id: null | string) => void {
  const pick = useSetAtom(pickInListAtom)
  const clear = useSetAtom(pickedItemAtom)
  return useCallback(
    (id: null | string) => {
      if (id === null) clear(null)
      else pick({ threadId: id, uid: null })
    },
    [pick, clear]
  )
}

/** The Send rows the list draws, after its status and search narrowing. */
export function useSendRows(): { all: SendRow[]; loading: boolean; rows: SendRow[] } {
  const enabled = useAtomValue(activeListAtom) === 'send'
  const status = useAtomValue(sendStatusFilterAtom)
  const query = useAtomValue(sendQueryAtom)
  const { data: messages, isLoading } = useSentMessagesQuery(enabled)
  const { data: sends } = useSendsQuery(null, enabled)

  const all = useMemo(() => joinSends(messages ?? [], sends ?? []), [messages, sends])
  const rows = useMemo(() => {
    const byStatus = filterByStatus(all, status)
    const q = query.trim().toLowerCase()
    if (!q) return byStatus
    return byStatus.filter(
      (r) => r.to.toLowerCase().includes(q) || r.subject.toLowerCase().includes(q)
    )
  }, [all, status, query])

  return { all, loading: isLoading, rows }
}
