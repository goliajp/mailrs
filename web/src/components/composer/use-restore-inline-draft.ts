import type { StructuredComposeHandle } from '@/components/structured-compose'

import { toast } from '@goliapkg/gds'
import { useEffect, useRef } from 'react'

import { useDraftsQuery } from '@/hooks/use-drafts'
import { useT } from '@/i18n'
import { localInputFromEpochSeconds } from '@/lib/send-mail'

import { type ComposerStart } from './start'

/**
 * The inline reply comes back to the draft this thread already has: its
 * body, its recipients when they were changed, and who it is from and when
 * it is set to go.
 */
export function useRestoreInlineDraft(
  start: ComposerStart,
  editorRef: React.RefObject<null | StructuredComposeHandle>,
  draftIdRef: React.RefObject<null | number>,
  lastSavedRef: React.RefObject<string>,
  set: {
    setBcc: (v: string) => void
    setCc: (v: string) => void
    setFrom: (v: string) => void
    setScheduledAt: (v: string) => void
    setTo: (v: string) => void
  }
) {
  const t = useT()
  const inline = start.kind === 'inline'
  const { data: drafts = [] } = useDraftsQuery(inline)
  const restored = useRef(false)
  const threadId = inlineThread(start)
  useEffect(() => {
    if (!inline || restored.current || drafts.length === 0) return
    const mine = drafts.find((d) => d.reply_to_thread_id === threadId)
    if (!mine || !mine.body.trim()) return
    restored.current = true
    draftIdRef.current = Number(mine.id)
    const scheduledAt = localInputFromEpochSeconds(mine.scheduled_at ?? null)
    // the same shape `saveNow` compares, so a restore is not taken for an edit
    lastSavedRef.current = JSON.stringify({
      bcc: mine.bcc ?? '',
      body: mine.body,
      cc: mine.cc ?? '',
      from: mine.from ?? '',
      scheduledAt,
      subject: mine.subject,
      to: mine.to,
    })
    if (mine.from) set.setFrom(mine.from)
    if (scheduledAt) set.setScheduledAt(scheduledAt)
    if (mine.to) set.setTo(mine.to)
    if (mine.cc) set.setCc(mine.cc)
    if (mine.bcc) set.setBcc(mine.bcc)
    setTimeout(() => {
      const handle = editorRef.current
      if (handle && !handle.getMarkdown().trim()) {
        handle.setMarkdown(mine.body)
        toast.info(t('compose.draftRestored'), { duration: 2000 })
      }
    }, 200)
  }, [drafts, draftIdRef, editorRef, inline, lastSavedRef, set, t, threadId])
}

function inlineThread(s: ComposerStart): string {
  if (s.kind === 'inline') return s.ctx.threadId
  return ''
}
