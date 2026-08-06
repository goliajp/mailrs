import { useAtomValue } from 'jotai'
import { useEffect, useMemo, useRef } from 'react'

import { useConversationRows } from '@/hooks/use-current-list'
import { useMarkReadMutation } from '@/hooks/use-mail-mutations'
import { crossAccountReadAtom, selectedDomainsAtom } from '@/store/ui'

/**
 * Mark a thread read because it is being read.
 *
 * The condition is that a reading pane is *showing* the thread — which
 * is why callers pass `showing` rather than this hook deriving it. A
 * thread can be selected while the phone is still on the list: the first
 * row is picked as soon as the list arrives, so "selected" would mark
 * mail read that has never been on screen.
 *
 * That is not hypothetical. Until 2026-08-05 this effect lived only in
 * the desktop `ThreadView`, and the desktop tree was mounted on phones
 * too — hidden, auto-selecting the first row, and marking it read. Just
 * opening the mailbox on a phone marked the newest thread read, twice,
 * measured in a production build. Removing that tree then removed the
 * only thing marking anything read on mobile, because the mobile reading
 * view never had this effect of its own. One behaviour, one place, and
 * both views say when they are showing something.
 *
 * @param threadId the thread on screen, or null
 * @param showing whether a reading pane is actually displaying it
 * @param onMarked notified when the mutation is issued, for local state
 */
export function useMarkReadOnOpen(
  threadId: null | string,
  showing: boolean,
  onMarked?: () => void
): void {
  const { rows } = useConversationRows()
  // A primitive, so an unrelated change to the list does not re-run this.
  const unreadCount = useMemo(() => {
    if (!threadId) return 0
    return rows.find((c) => c.thread_id === threadId)?.unread_count ?? 0
  }, [threadId, rows])

  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const domainsRef = useRef(selectedDomains)
  domainsRef.current = selectedDomains
  const crossAccountRead = useAtomValue(crossAccountReadAtom)
  const crossAccountReadRef = useRef(crossAccountRead)
  crossAccountReadRef.current = crossAccountRead
  const onMarkedRef = useRef(onMarked)
  onMarkedRef.current = onMarked

  const markReadMutation = useMarkReadMutation()

  useEffect(() => {
    if (!showing || !threadId) return
    // Already read — nothing to do.
    if (unreadCount === 0) return
    // Mutation in flight — the ONLY re-entry guard needed. The wrapper
    // flips pending true→false several times per successful cycle
    // (onMutate → onSuccess → onSettled) and the mutation object is a
    // dep, so without this the POST re-issues on every micro-transition.
    // On completion the optimistic patch has set unread_count = 0, so the
    // guard above returns first. If it errors the patch stays (see
    // `useMarkReadMutation`), so there is no retry loop either.
    if (markReadMutation.isPending) return

    const doms = domainsRef.current
    const crossAll = crossAccountReadRef.current
    onMarkedRef.current?.()
    markReadMutation.mutate({
      domains: crossAll && doms.length > 0 ? doms : undefined,
      threadId,
    })
  }, [threadId, showing, unreadCount, markReadMutation])
}
