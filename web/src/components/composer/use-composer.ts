import type { ComposerCarry, ComposerStart } from './start'
import type { StructuredComposeHandle } from '@/components/structured-compose'
import type { WireTemplate } from '@/wire/schemas/templates'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { useCallback, useEffect, useRef, useState } from 'react'

import { useAutosaveStatus } from '@/hooks/use-autosave-status'
import { useDefaultSignature } from '@/hooks/use-default-signature'
import { useDeleteDraftMutation, useDraftsQuery, useSaveDraftMutation } from '@/hooks/use-drafts'
import { applyOptimisticSent } from '@/hooks/use-mail-mutations'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { epochSecondsFromLocalInput, parseAddressList, sendMail } from '@/lib/send-mail'
import { authAtom } from '@/store/auth'
import { wireDeletePendingSend } from '@/wire/endpoints/mail'
import { wireListTemplates } from '@/wire/endpoints/templates'

import {
  initialBcc,
  initialBody,
  initialCc,
  initialDraftId,
  initialSubject,
  initialTo,
  inReplyToOf,
  isForward,
  isPrefilled,
  isReplyLike,
  modeSubject,
  modeTo,
  replyThreadOf,
  sentLabel,
} from './start'

export type Composer = ReturnType<typeof useComposer>

type Options = {
  onSent: () => void
  /** Kept files of a re-edited send, by index; the server re-extracts them. */
  redraftKeep?: number[]
}

/** The login address first, then what the server lets this account send as. */
export function sendAddresses(own: string | undefined, sendAs: string[] | undefined): string[] {
  const out: string[] = []
  for (const a of [own ?? '', ...(sendAs ?? [])]) {
    const v = a.trim()
    if (v && !out.some((o) => o.toLowerCase() === v.toLowerCase())) out.push(v)
  }
  return out
}

/**
 * One compose session: the fields, the server draft, sending and undoing a
 * send. Every shell — page, floating window, inline reply — drives one of
 * these, so drafts, undo and a failed send behave the same in all three.
 */
export function useComposer(start: ComposerStart, opts: Options) {
  const t = useT()
  const auth = useAtomValue(authAtom)
  const editorRef = useRef<StructuredComposeHandle>(null)
  // from the server, which is where Settings → Signatures saves it
  const { html: signature } = useDefaultSignature()

  const [to, setTo] = useState(() => initialTo(start))
  const [cc, setCc] = useState(() => initialCc(start))
  const [bcc, setBcc] = useState(() => initialBcc(start))
  const [showCcBcc, setShowCcBcc] = useState(() => Boolean(initialCc(start) || initialBcc(start)))
  const [subject, setSubject] = useState(() => initialSubject(start))
  const [from, setFrom] = useState(() => carriedFrom(start) || (auth?.address ?? ''))
  const [scheduledAt, setScheduledAt] = useState(() => carriedSchedule(start))
  const [sending, setSending] = useState(false)
  const [error, setError] = useState('')

  const fromOptions = sendAddresses(auth?.address, auth?.send_as)

  // the inline reply's mode is its parent's: switching it starts the
  // recipients and subject over for the new mode
  const mode = start.kind === 'inline' ? start.mode : null
  const lastMode = useRef(mode)
  useEffect(() => {
    if (start.kind !== 'inline' || lastMode.current === start.mode) return
    lastMode.current = start.mode
    setTo(modeTo(start.ctx, start.mode))
    setSubject(modeSubject(start.ctx.subject, start.mode))
    setError('')
  }, [start])

  const { data: templates = [] } = useQuery({
    queryKey: mailKeys.templates(),
    staleTime: 5 * 60 * 1000,
    queryFn: ({ signal }) => wireListTemplates(signal),
  })

  // server draft: the first save allocates an id, later saves reuse it,
  // a send deletes it; `sent` stops a trailing tick from bringing it back
  const draftIdRef = useRef<null | number>(initialDraftId(start))
  const sentRef = useRef(false)
  const lastSavedRef = useRef('')
  const saveDraftMut = useSaveDraftMutation()
  const deleteDraftMut = useDeleteDraftMutation()
  const autosave = useAutosaveStatus()
  const latest = useRef({ bcc, cc, subject, to })
  latest.current = { bcc, cc, subject, to }
  const prefilled = isPrefilled(start)
  const threadId = replyThreadOf(start)

  const saveNow = useCallback(async () => {
    if (sentRef.current) return
    const body = editorRef.current?.getMarkdown() ?? ''
    const { bcc, cc, subject, to } = latest.current
    const fieldsTyped = !prefilled && Boolean(to.trim() || subject.trim())
    if (!body.trim() && !fieldsTyped) return
    const snapshot = JSON.stringify({ bcc, body, cc, subject, to })
    if (snapshot === lastSavedRef.current) return
    // a failure retries on the next tick, and says so once retrying has
    // stopped being an explanation — see use-autosave-status
    await autosave.record(async () => {
      const res = await saveDraftMut.mutateAsync({
        bcc,
        body,
        cc,
        id: draftIdRef.current ?? undefined,
        reply_to_thread_id: threadId,
        subject,
        to,
      })
      if (res.id !== undefined) draftIdRef.current = Number(res.id)
      lastSavedRef.current = snapshot
    })
  }, [autosave, prefilled, saveDraftMut, threadId])

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

  useRestoreInlineDraft(start, editorRef, draftIdRef, lastSavedRef, { setBcc, setCc, setTo })

  const send = async () => {
    if (sending) return
    const recipients = parseAddressList(to)
    if (recipients.length === 0) {
      setError(t('compose.needRecipient'))
      return
    }
    const content = editorRef.current?.getContent()
    if (!content || !content.compose.text.trim()) {
      setError(t('compose.needBody'))
      return
    }
    setError('')
    setSending(true)
    try {
      const forward = forwardSource(start)
      // forwarding through the server with nothing attached here: the body
      // carries only what was typed, and the server appends the original
      const viaServer = forward !== null && content.attachments.length === 0
      const result = await sendMail({
        attachments: content.attachments,
        bcc: parseAddressList(bcc),
        body: pick(viaServer, content.compose.text, content.fullText),
        cc: parseAddressList(cc),
        forwardAttachmentsFrom: forward?.uid,
        forwardMessageId: forward?.messageId,
        from,
        htmlBody: pick(viaServer, content.compose.html, content.fullHtml),
        inReplyTo: inReplyToOf(start),
        redraftKeep: opts.redraftKeep,
        redraftOf: redraftOf(start),
        replyToThreadId: threadId,
        scheduledAt: epochSecondsFromLocalInput(scheduledAt),
        subject,
        to: recipients,
      })
      if (!result.success) {
        setError(result.message ?? t('compose.sendFailed'))
        return
      }
      const sentId = result.message_id
      if (sentId) {
        applyOptimisticSent({
          message_id: sentId,
          subject,
          thread_id: threadId ?? '',
          to: recipients.join(', '),
        })
      }
      toast.success(t(sentLabel(start)), undoOption(sentId, t))
      sentRef.current = true
      const draftId = draftIdRef.current
      draftIdRef.current = null
      if (draftId !== null) deleteDraftMut.mutate(draftId)
      void queryClient.invalidateQueries({ queryKey: mailKeys.conversations() })
      editorRef.current?.clearCompose()
      setScheduledAt('')
      lastSavedRef.current = ''
      // the inline reply stays open for the next one; every other shell closes
      sentRef.current = !prefilled
      opts.onSent()
    } catch (err) {
      void saveNow()
      toast.error(errorText(err, t('compose.networkDraftSaved')), {
        action: { label: t('common.retry'), onClick: () => void send() },
        duration: 10000,
      })
    } finally {
      setSending(false)
    }
  }

  const applyTemplate = (tpl: WireTemplate) => {
    if (tpl.subject && !isReplyLike(start)) setSubject(tpl.subject)
    if (tpl.text_body) editorRef.current?.setMarkdown(tpl.text_body)
    else if (tpl.html_body) editorRef.current?.setComposeContent(tpl.html_body)
  }

  /** The session as it stands, for a floating window expanded to the page. */
  const carry = async (): Promise<ComposerCarry> => {
    await saveNow()
    return {
      bcc,
      body: editorRef.current?.getMarkdown() ?? '',
      cc,
      draftId: draftIdRef.current,
      from,
      replyToThreadId: threadId ?? null,
      scheduledAt,
      subject,
      to,
    }
  }

  return {
    applyTemplate,
    autosave,
    bcc,
    carry,
    cc,
    editorRef,
    error,
    from,
    fromOptions,
    mode,
    saveNow,
    scheduledAt,
    send,
    sending,
    setBcc,
    setCc,
    setError,
    setFrom,
    setScheduledAt,
    setShowCcBcc,
    setSubject,
    setTo,
    showCcBcc,
    signature,
    start,
    subject,
    templates,
    to,
  }
}

function carriedFrom(s: ComposerStart): string {
  if (s.kind === 'carry') return s.carry.from
  return ''
}

function carriedSchedule(s: ComposerStart): string {
  if (s.kind === 'carry') return s.carry.scheduledAt
  return ''
}

function forwardSource(s: ComposerStart): null | { messageId?: string; uid?: number } {
  if (!isForward(s) || s.kind !== 'inline') return null
  const messageId = s.ctx.forwardMessageId || undefined
  const uid = positive(s.ctx.forwardAttachmentsUid)
  if (!messageId && uid === undefined) return null
  return { messageId, uid }
}

function pick(cond: boolean, yes: string, no: string): string {
  if (cond) return yes
  return no
}

function positive(n: null | number): number | undefined {
  if (n !== null && n > 0) return n
  return undefined
}

function redraftOf(s: ComposerStart): string | undefined {
  if (s.kind === 'redraft') return s.redraft.redraftOf
  return undefined
}

/** The toast's Undo, while the send is still held back on the server. */
function undoOption(sentId: string | undefined, t: ReturnType<typeof useT>) {
  if (!sentId) return {}
  return {
    action: {
      label: t('common.undo'),
      onClick: async () => {
        try {
          await wireDeletePendingSend(sentId)
          toast.success(t('compose.sendCancelled'))
        } catch {
          toast.error(t('compose.undoTooLate'))
        }
      },
    },
    duration: 8000,
  }
}

/**
 * The inline reply comes back to the draft this thread already has: its
 * body, and its recipients when they were changed.
 */
function useRestoreInlineDraft(
  start: ComposerStart,
  editorRef: React.RefObject<null | StructuredComposeHandle>,
  draftIdRef: React.RefObject<null | number>,
  lastSavedRef: React.RefObject<string>,
  set: { setBcc: (v: string) => void; setCc: (v: string) => void; setTo: (v: string) => void }
) {
  const t = useT()
  const inline = start.kind === 'inline'
  const { data: drafts = [] } = useDraftsQuery(inline)
  const restored = useRef(false)
  const threadId = start.kind === 'inline' ? start.ctx.threadId : ''
  useEffect(() => {
    if (!inline || restored.current || drafts.length === 0) return
    const mine = drafts.find((d) => d.reply_to_thread_id === threadId)
    if (!mine || !mine.body.trim()) return
    restored.current = true
    draftIdRef.current = Number(mine.id)
    lastSavedRef.current = JSON.stringify({
      bcc: mine.bcc ?? '',
      body: mine.body,
      cc: mine.cc ?? '',
      subject: mine.subject,
      to: mine.to,
    })
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
