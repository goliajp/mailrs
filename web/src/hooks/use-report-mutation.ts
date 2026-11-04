import type { ThreadMessage } from '@/lib/types'
import type { WireReportOutcome } from '@/wire/schemas/report'

import { toast } from '@goliapkg/gds'
import { useMutation } from '@tanstack/react-query'
import { getDefaultStore } from 'jotai'

import { threadQueryOptions } from '@/hooks/use-mail-queries'
import { tNow } from '@/i18n'
import { invalidateBucketMove } from '@/lib/mail-cache'
import { queryClient } from '@/lib/query-client'
import { authAtom } from '@/store/auth'
import { wireReportMessage } from '@/wire/endpoints/report'

/** What the reporter is told once the server has decided. */
export function outcomeText(o: WireReportOutcome): string {
  const base = stateText(o)
  if (o.own_held === 0) return base
  return tNow('action.report.withOwn', { base, count: o.own_held })
}

/**
 * The message a report of `messages` names: the newest one somebody else
 * wrote. A conversation of only the reader's own mail has none.
 */
export function reportedMessage(messages: readonly ThreadMessage[], me: string): null | string {
  const mine = me.toLowerCase()
  const theirs = messages.filter((m) => !m.sender.toLowerCase().includes(mine))
  const newest = [...theirs].sort((a, b) => b.internal_date - a.internal_date)[0]
  return newest?.message_id ?? null
}

/**
 * Report a conversation as a scam. The server holds the reader's copy at
 * once, so the conversation leaves every list for Review; the toast says
 * what became of the other copies.
 */
export function useReportMutation() {
  return useMutation<WireReportOutcome, Error, { threadId: string }>({
    mutationFn: async ({ threadId }) => {
      const messages = await queryClient.fetchQuery(threadQueryOptions(threadId, []))
      const me = getDefaultStore().get(authAtom)?.address ?? ''
      const messageId = reportedMessage(messages, me)
      if (!messageId) throw new Error(tNow('action.failed'))
      return wireReportMessage(messageId)
    },
    onSettled: () => invalidateBucketMove(),
    onSuccess: (o) => {
      toast.success(outcomeText(o), { duration: 10000 })
    },
  })
}

function stateText(o: WireReportOutcome): string {
  switch (o.state) {
    case 'dismissed':
      return tNow('action.report.dismissed')
    case 'none_found':
      if (o.no_fingerprint) return tNow('action.report.noFingerprint')
      return tNow('action.report.noneFound')
    case 'taken_back':
      return tNow('action.report.takenBack', { count: o.taken_back })
    case 'waiting':
      return tNow('action.report.waiting')
  }
}
