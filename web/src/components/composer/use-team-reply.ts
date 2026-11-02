import type { SendResult, SharedReply } from '@/lib/send-mail'

import { useSetAtom } from 'jotai'
import { useEffect, useMemo, useRef } from 'react'

import { teamComposingAtom, teamConvOf, useTeamOverview } from '@/hooks/use-team'
import { useT } from '@/i18n'
import { formatDate } from '@/lib/format'
import { wireAnnounceDraft } from '@/wire/endpoints/shared'

/**
 * A reply to a team inbox conversation. The team's reply count is
 * remembered when the composer opens; sending names it, and the server
 * refuses the send if another member answered since. Saving the draft
 * tells the team who is writing.
 */
export function useTeamReply(threadId: null | string | undefined) {
  const t = useT()
  const c = useMemo(() => teamConvOf(threadId), [threadId])
  const { data } = useTeamOverview(c)
  const seen = useRef<null | number>(null)
  const setComposing = useSetAtom(teamComposingAtom)
  useEffect(() => {
    if (data && seen.current === null) seen.current = data.reply.seq
  }, [data])
  useEffect(() => () => setComposing(null), [setComposing])

  /** What the send names, or null for an ordinary reply. */
  const shared = (force: boolean): null | SharedReply => {
    if (!c) return null
    return { ...c, force, seenSeq: seen.current ?? data?.reply.seq ?? 0 }
  }

  /** Ask whether to send a second reply after someone else's. */
  const confirmSecond = (r: NonNullable<SendResult['alreadyReplied']>): boolean =>
    window.confirm(t('team.replied.confirm', { name: r.by, time: formatDate(r.at) }))

  /** The draft was saved: the team sees who is writing. */
  const saved = (draftId: null | number) => {
    if (!c || draftId === null) return
    setComposing(`${c.gid}:${c.conv}`)
    void wireAnnounceDraft(c, draftId).catch(() => {})
  }

  /** Sent: the next reply starts from what is current then. */
  const sent = () => {
    seen.current = null
    setComposing(null)
  }

  return { confirmSecond, saved, sent, shared }
}
