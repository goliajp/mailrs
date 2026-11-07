import type { ComposerStart } from './start'
import type { DraftFiles } from './use-draft-files'
import type { StructuredComposeHandle } from '@/components/structured-compose'
import type { WireDraftAttachment } from '@/wire/schemas/mail'

import { useCallback, useEffect, useRef } from 'react'

import { useAutosaveStatus } from '@/hooks/use-autosave-status'
import { useSaveDraftMutation } from '@/hooks/use-drafts'
import { epochSecondsFromLocalInput } from '@/lib/send-mail'

import {
  initialAttachments,
  initialBody,
  initialDraftId,
  isPrefilled,
  replyThreadOf,
} from './start'
import { filesKey } from './use-draft-files'

export type DraftFields = {
  bcc: string
  cc: string
  from: string
  scheduledAt: string
  subject: string
  to: string
}

/**
 * What a save compares to tell an edit from nothing new; a restored draft
 * builds the same shape so coming back to it is not taken for an edit.
 */
export function draftSnapshot(
  fields: DraftFields & { attachments: readonly WireDraftAttachment[]; body: string }
): string {
  const { attachments, bcc, body, cc, from, scheduledAt, subject, to } = fields
  return JSON.stringify({
    attachments: filesKey(attachments),
    bcc,
    body,
    cc,
    from,
    scheduledAt,
    subject,
    to,
  })
}

/**
 * The server draft of a session: the first save allocates an id, later
 * saves reuse it, a send deletes it; `sent` stops a trailing tick from
 * bringing it back. Saved every three seconds while something changed.
 * Every save names the session's uploaded files, so the draft holds what
 * the composer shows.
 */
export function useComposerDraft(
  start: ComposerStart,
  editorRef: React.RefObject<null | StructuredComposeHandle>,
  fields: DraftFields,
  files: DraftFiles,
  onSaved: (draftId: null | number) => void
) {
  const draftIdRef = useRef<null | number>(initialDraftId(start))
  const sentRef = useRef(false)
  const lastSavedRef = useRef('')
  // the files the server's draft names, as of the last save that landed
  const savedFilesRef = useRef(filesKey(initialAttachments(start)))
  const saveDraftMut = useSaveDraftMutation()
  const autosave = useAutosaveStatus()
  const latest = useRef({ ...fields, attachments: files.stored })
  latest.current = { ...fields, attachments: files.stored }
  const prefilled = isPrefilled(start)
  const threadId = replyThreadOf(start)

  const saveNow = useCallback(async () => {
    if (sentRef.current) return
    const body = editorRef.current?.getMarkdown() ?? ''
    const { attachments, bcc, cc, from, scheduledAt, subject, to } = latest.current
    const fieldsTyped = !prefilled && Boolean(to.trim() || subject.trim())
    if (!body.trim() && !fieldsTyped && attachments.length === 0) return
    const snapshot = draftSnapshot({ ...latest.current, body })
    if (snapshot === lastSavedRef.current) return
    // a failure retries on the next tick, and says so once retrying has
    // stopped being an explanation — see use-autosave-status
    await autosave.record(async () => {
      const res = await saveDraftMut.mutateAsync({
        attachments,
        bcc,
        body,
        cc,
        from,
        id: draftIdRef.current ?? undefined,
        reply_to_thread_id: threadId,
        scheduled_at: epochSecondsFromLocalInput(scheduledAt) ?? undefined,
        subject,
        to,
      })
      if (res.id !== undefined) draftIdRef.current = Number(res.id)
      lastSavedRef.current = snapshot
      savedFilesRef.current = filesKey(attachments)
      onSaved(draftIdRef.current)
    })
  }, [autosave, editorRef, onSaved, prefilled, saveDraftMut, threadId])

  const saveNowRef = useRef(saveNow)
  saveNowRef.current = saveNow
  useEffect(() => {
    const timer = setInterval(() => void saveNowRef.current(), 3000)
    return () => clearInterval(timer)
  }, [])

  // a reopened session's body goes in once the lazy editor has mounted
  useEffect(() => {
    const body = initialBody(start)
    if (!body) return
    const timer = setTimeout(() => editorRef.current?.setMarkdown(body), 250)
    return () => clearTimeout(timer)
    // the start is read once, as the fields' initial state is
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return {
    autosave,
    draftIdRef,
    lastSavedRef,
    prefilled,
    savedFilesRef,
    saveNow,
    sentRef,
    threadId,
  }
}
