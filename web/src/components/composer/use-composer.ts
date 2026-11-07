import type { ComposerCarry, ComposerStart } from './start'
import type { StructuredComposeHandle } from '@/components/structured-compose'
import type { WireTemplate } from '@/wire/schemas/templates'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { useEffect, useRef, useState } from 'react'

import { useDefaultSignature } from '@/hooks/use-default-signature'
import { applyOptimisticSent } from '@/hooks/use-mail-mutations'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import {
  epochSecondsFromLocalInput,
  localInputFromEpochSeconds,
  parseAddressList,
  sendMail,
} from '@/lib/send-mail'
import { authAtom } from '@/store/auth'
import { conversationKeys } from '@/store/query-keys-v21'
import { wireDeletePendingSend } from '@/wire/endpoints/mail'
import { wireListTemplates } from '@/wire/endpoints/templates'

import {
  initialAttachments,
  initialBcc,
  initialCc,
  initialSubject,
  initialTo,
  inReplyToOf,
  isForward,
  isReplyLike,
  modeSubject,
  modeTo,
  replyThreadOf,
  sentLabel,
} from './start'
import { useComposerDraft } from './use-composer-draft'
import { filesKey, useDraftFiles } from './use-draft-files'
import { useReplyMask } from './use-reply-mask'
import { useRestoreInlineDraft } from './use-restore-inline-draft'
import { useTeamReply } from './use-team-reply'

export type Composer = ReturnType<typeof useComposer>

type Options = {
  onSent: () => void
  /** Kept files of a re-edited send, by index; the server re-extracts them. */
  redraftKeep?: null | number[]
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

  const team = useTeamReply(replyThreadOf(start))
  const replyMask = useReplyMask(start, from, auth?.address ?? '', { setCc, setFrom, setTo })
  const fromOptions = sendAddresses(auth?.address, [
    ...(auth?.send_as ?? []),
    ...[replyMask].filter((m) => m !== ''),
  ])

  // the inline reply's mode is its parent's: switching it starts the
  // recipients and subject over for the new mode
  const mode = inlineMode(start)
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

  const files = useDraftFiles(initialAttachments(start))
  const {
    autosave,
    draftIdRef,
    lastSavedRef,
    prefilled,
    savedFilesRef,
    saveNow,
    sentRef,
    threadId,
  } = useComposerDraft(
    start,
    editorRef,
    { bcc, cc, from, scheduledAt, subject, to },
    files,
    team.saved
  )

  useRestoreInlineDraft(start, editorRef, draftIdRef, lastSavedRef, savedFilesRef, {
    setBcc,
    setCc,
    setFiles: files.replace,
    setFrom,
    setScheduledAt,
    setTo,
  })

  const send = async (force = false): Promise<void> => {
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
      // the server attaches the draft's files: they must be the ones shown
      await saveNow()
      if (filesKey(files.stored) !== savedFilesRef.current) {
        setError(t('compose.filesNotSaved'))
        return
      }
      const forward = forwardSource(start)
      // forwarding through the server with nothing attached here: the body
      // carries only what was typed, and the server appends the original
      const viaServer = forward !== null && files.all.length === 0
      const draftId = draftIdRef.current
      const result = await sendMail({
        // files the server has are in the draft; only the rest go in the form
        attachments: files.unsaved,
        bcc: parseAddressList(bcc),
        body: pick(viaServer, content.compose.text, content.fullText),
        cc: parseAddressList(cc),
        draftId,
        forwardAttachmentsFrom: forward?.uid,
        forwardMessageId: forward?.messageId,
        from,
        htmlBody: pick(viaServer, content.compose.html, content.fullHtml),
        inReplyTo: inReplyToOf(start),
        redraftKeep: opts.redraftKeep,
        redraftOf: redraftOf(start),
        replyToThreadId: threadId,
        scheduledAt: epochSecondsFromLocalInput(scheduledAt),
        shared: team.shared(force),
        subject,
        to: recipients,
      })
      if (result.alreadyReplied) {
        setSending(false)
        if (team.confirmSecond(result.alreadyReplied)) return send(true)
        setError(t('team.replied.cancelled', { name: result.alreadyReplied.by }))
        return
      }
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
      team.sent()
      sentRef.current = true
      // the server deleted the draft it sent
      draftIdRef.current = null
      savedFilesRef.current = ''
      void queryClient.invalidateQueries({ queryKey: mailKeys.drafts() })
      void queryClient.invalidateQueries({ queryKey: conversationKeys.all() })
      editorRef.current?.clearCompose()
      files.replace([])
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
      attachments: files.stored,
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
    files,
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
  if (s.kind === 'draft') return s.draft.from
  return ''
}

function carriedSchedule(s: ComposerStart): string {
  if (s.kind === 'carry') return s.carry.scheduledAt
  if (s.kind === 'draft') return localInputFromEpochSeconds(s.draft.scheduledAt)
  return ''
}

function forwardSource(s: ComposerStart): null | { messageId?: string; uid?: number } {
  if (!isForward(s) || s.kind !== 'inline') return null
  const messageId = s.ctx.forwardMessageId || undefined
  const uid = positive(s.ctx.forwardAttachmentsUid)
  if (!messageId && uid === undefined) return null
  return { messageId, uid }
}

function inlineMode(s: ComposerStart) {
  if (s.kind === 'inline') return s.mode
  return null
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
