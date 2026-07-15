import type { TemplateInfo } from './types'
import type { StructuredComposeHandle } from '@/components/structured-compose'
import type { ComposeDraftSource, ComposeReplySource } from '@/store/ui'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useAtomValue, useSetAtom } from 'jotai'
import { X } from 'lucide-react'
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'

import { useDeleteDraftMutation, useSaveDraftMutation } from '@/hooks/use-drafts'
import { formatFullDate } from '@/lib/format'
import { escapeHtml } from '@/lib/html-utils'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { parseAddressList, sendMail } from '@/lib/send-mail'
import { authAtom, getToken } from '@/store/auth'
import { signatureAtom, signatureEnabledAtom } from '@/store/settings'
import { composeDraftSourceAtom, composeReplySourceAtom, composingNewAtom } from '@/store/ui'
import { adminListGet } from '@/wire/endpoints/admin'
import { wireGenerateSubject, wirePolishText } from '@/wire/endpoints/ai'
import { wireDeletePendingSend } from '@/wire/endpoints/mail'

import { ActionBar } from './action-bar'
import { AddressFields } from './address-fields'

const StructuredCompose = lazy(() =>
  import('@/components/structured-compose').then((m) => ({ default: m.StructuredCompose }))
)

export function NewConversation() {
  const auth = useAtomValue(authAtom)
  const signature = useAtomValue(signatureAtom)
  const signatureEnabled = useAtomValue(signatureEnabledAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const replySource = useAtomValue(composeReplySourceAtom)
  const setReplySource = useSetAtom(composeReplySourceAtom)
  const draftSource = useAtomValue(composeDraftSourceAtom)
  const setDraftSource = useSetAtom(composeDraftSourceAtom)
  const isReply = replySource !== null

  // the server draft id for this compose session. starts from a reopened
  // draft, otherwise null until the first autosave allocates one.
  const draftIdRef = useRef<null | number>(draftSource?.id ?? null)
  // set once the message is sent so a trailing autosave / close can't
  // resurrect the just-deleted draft.
  const sentRef = useRef(false)
  const saveDraftMut = useSaveDraftMutation()
  const deleteDraftMut = useDeleteDraftMutation()

  const [to, setTo] = useState(() => initialTo(draftSource, replySource))
  const [cc, setCc] = useState(() => draftSource?.cc ?? '')
  const [bcc, setBcc] = useState(() => draftSource?.bcc ?? '')
  const [showCcBcc, setShowCcBcc] = useState(() => Boolean(draftSource?.cc || draftSource?.bcc))
  const [subject, setSubject] = useState(() => initialSubject(draftSource, replySource))
  const [sending, setSending] = useState(false)
  const [polishing, setPolishing] = useState(false)
  const [generatingSubject, setGeneratingSubject] = useState(false)
  const [error, setError] = useState('')
  const [scheduledAt, setScheduledAt] = useState('')
  const [showSchedulePicker, setShowSchedulePicker] = useState(false)
  const composeRef = useRef<StructuredComposeHandle>(null)
  const { data: templates = [] } = useQuery({
    queryKey: mailKeys.templates(),
    staleTime: 5 * 60 * 1000,
    queryFn: () => adminListGet<TemplateInfo>('/mail/templates'),
  })

  // latest field values, read by the bind-once autosave interval below.
  const latest = useRef({ bcc, cc, subject, to })
  latest.current = { bcc, cc, subject, to }

  // upsert the current compose state as a server draft, deduped so an
  // unchanged tick is a no-op. the first save allocates an id; later
  // saves reuse it via draftIdRef.
  const lastSavedRef = useRef('')
  const saveNow = useCallback(async () => {
    if (sentRef.current) return
    const body = composeRef.current?.getMarkdown() ?? ''
    const { bcc, cc, subject, to } = latest.current
    if (!to.trim() && !subject.trim() && !body.trim()) return
    const snapshot = JSON.stringify({ bcc, body, cc, subject, to })
    if (snapshot === lastSavedRef.current) return
    try {
      const res = await saveDraftMut.mutateAsync({
        bcc,
        body,
        cc,
        id: draftIdRef.current ?? undefined,
        reply_to_thread_id: replySource?.threadId,
        subject,
        to,
      })
      if (res.id !== undefined) draftIdRef.current = Number(res.id)
      lastSavedRef.current = snapshot
    } catch {
      // transient — the next interval tick retries
    }
  }, [replySource, saveDraftMut])

  // bind the interval once; read the latest saveNow through a ref.
  const saveNowRef = useRef(saveNow)
  saveNowRef.current = saveNow
  useEffect(() => {
    const timer = setInterval(() => void saveNowRef.current(), 3000)
    return () => clearInterval(timer)
  }, [])

  // prefill the editor body when reopening a saved draft (the editor is
  // lazy — wait a tick for it to mount, like the reply-box restore).
  useEffect(() => {
    if (!draftSource?.body) return
    const t = setTimeout(() => composeRef.current?.setMarkdown(draftSource.body), 250)
    return () => clearTimeout(t)
  }, [draftSource])

  // this compose session's draft source is consumed on mount; clear it so
  // the next fresh compose doesn't reopen it.
  useEffect(() => {
    return () => setDraftSource(null)
  }, [setDraftSource])

  // composer opens by flipping an atom, which doesn't push a history entry.
  // without this, browser Back leaves /mail entirely. push a sentinel on
  // mount so Back can pop it.
  useEffect(() => {
    window.history.pushState({ composerSentinel: true }, '')

    const onPop = () => {
      setComposingNew(false)
      setReplySource(null)
    }
    window.addEventListener('popstate', onPop)

    return () => {
      window.removeEventListener('popstate', onPop)
    }
  }, [setComposingNew, setReplySource])

  const applyTemplate = (t: TemplateInfo) => {
    setSubject(t.subject)
    if (t.text_body) composeRef.current?.setMarkdown(t.text_body)
    else if (t.html_body) composeRef.current?.setComposeContent(t.html_body)
  }

  const polish = async () => {
    const handle = composeRef.current
    if (!handle) return
    const text = handle.getMarkdown()
    if (!text.trim()) return
    setPolishing(true)
    try {
      const result = await wirePolishText(text)
      if (result.success && result.polished) {
        handle.setMarkdown(result.polished)
        toast.success('Text polished')
      } else {
        toast.error(result.message ?? 'Polish failed')
      }
    } catch {
      toast.error('AI unavailable')
    } finally {
      setPolishing(false)
    }
  }

  const generateSubject = async () => {
    const content = composeRef.current?.getContent()
    if (!content?.compose.text.trim()) {
      toast.error('Write some content first')
      return
    }
    setGeneratingSubject(true)
    try {
      const result = await wireGenerateSubject({
        body: content.compose.text,
        context: to ? `To: ${to}` : undefined,
      })
      if (result.success && result.subject) {
        setSubject(result.subject)
        toast.success('Subject generated')
      } else {
        toast.error(result.message ?? 'Failed')
      }
    } catch {
      toast.error('AI unavailable')
    } finally {
      setGeneratingSubject(false)
    }
  }

  const closeComposer = () => {
    // persist whatever's typed as a draft before leaving (no-op if empty
    // or already sent). the request outlives this component's unmount.
    void saveNow()
    // when our sentinel is still on top, pop it so Back afterwards lands on
    // the thread underneath. popstate will re-run the close handlers, which
    // is idempotent.
    const state = window.history.state as null | { composerSentinel?: boolean }
    if (state?.composerSentinel) {
      window.history.back()
      return
    }
    setComposingNew(false)
    setReplySource(null)
  }

  const send = async () => {
    if (sending) return
    const recipients = parseAddressList(to)
    if (recipients.length === 0) {
      setError('Recipient is required')
      return
    }
    const content = composeRef.current?.getContent()
    if (!content || !content.compose.text.trim()) {
      setError('Message body is required')
      return
    }

    setError('')
    setSending(true)
    try {
      const result = await sendMail({
        attachments: content.attachments,
        bcc: parseAddressList(bcc),
        body: content.fullText,
        cc: parseAddressList(cc),
        from: auth?.address ?? '',
        htmlBody: content.fullHtml,
        inReplyTo: replySource?.messageId,
        scheduledAt: scheduledAt ? new Date(scheduledAt).toISOString() : undefined,
        subject,
        to: recipients,
        token: getToken() ?? '',
      })

      if (result.success) {
        const sentMessageId = result.message_id
        toast.success('Message sent', {
          ...(sentMessageId
            ? {
                action: {
                  label: 'Undo',
                  onClick: async () => {
                    try {
                      await wireDeletePendingSend(sentMessageId)
                      toast.success('Send cancelled')
                    } catch {
                      toast.error('Could not cancel — already delivered')
                    }
                  },
                },
                duration: 8000,
              }
            : {}),
        })
        // the message is out — drop the autosaved draft (best effort) and
        // block any trailing autosave from recreating it.
        sentRef.current = true
        if (draftIdRef.current !== null) {
          await deleteDraftMut.mutateAsync(draftIdRef.current).catch(() => undefined)
          draftIdRef.current = null
        }
        await queryClient.invalidateQueries({ queryKey: mailKeys.conversations() })
        closeComposer()
      } else {
        setError(result.message ?? 'Send failed')
      }
    } catch {
      setError('Network error')
    } finally {
      setSending(false)
    }
  }

  return (
    <div className="flex flex-1 flex-col">
      {/* header — subtle, not shouty */}
      <div className="border-border flex shrink-0 items-center justify-between border-b px-4 py-2">
        <span className="text-fg-muted text-xs font-medium">
          {isReply ? 'Reply' : 'New message'}
        </span>
        <button
          aria-label="Close"
          className="text-fg-muted hover:bg-bg-secondary rounded-md p-1 transition-colors"
          onClick={closeComposer}
          type="button"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {error && (
        <div
          className="bg-danger/10 text-danger mx-4 mt-2 rounded-md px-3 py-1.5 text-xs"
          role="alert"
        >
          {error}
        </div>
      )}

      <AddressFields
        bcc={bcc}
        cc={cc}
        generatingSubject={generatingSubject}
        onBccChange={setBcc}
        onCcChange={setCc}
        onGenerateSubject={generateSubject}
        onShowCcBcc={() => setShowCcBcc(true)}
        onSubjectChange={setSubject}
        onToChange={setTo}
        sending={sending}
        showCcBcc={showCcBcc}
        subject={subject}
        to={to}
      />

      {/* block-based composer */}
      <div className="min-h-0 flex-1">
        <Suspense fallback={<ComposerSkeleton />}>
          <StructuredCompose
            disabled={sending}
            mode={isReply ? 'reply' : 'new'}
            onSubmit={send}
            placeholder={isReply ? 'Type a reply...' : 'Write your message...'}
            quotedHeader={
              replySource
                ? `On ${formatFullDate(replySource.internalDate)}, ${replySource.sender} wrote:\n\n`
                : undefined
            }
            quotedHeaderHtml={
              replySource
                ? `<p style="color:#888">On ${escapeHtml(formatFullDate(replySource.internalDate))}, ${escapeHtml(replySource.sender)} wrote:</p>`
                : undefined
            }
            quotedHtml={
              replySource ? (replySource.htmlBody ?? replySource.textBody ?? undefined) : undefined
            }
            ref={composeRef}
            signature={signature}
            signatureEnabled={signatureEnabled}
          />
        </Suspense>
      </div>

      <ActionBar
        onCancel={closeComposer}
        onPolish={polish}
        onScheduleChange={setScheduledAt}
        onScheduleClear={() => {
          setScheduledAt('')
          setShowSchedulePicker(false)
        }}
        onSend={send}
        onTemplateSelect={applyTemplate}
        onToggleSchedulePicker={() => setShowSchedulePicker((v) => !v)}
        polishing={polishing}
        scheduledAt={scheduledAt}
        sending={sending}
        showSchedulePicker={showSchedulePicker}
        templates={templates}
      />
    </div>
  )
}

function ComposerSkeleton() {
  return (
    <div aria-busy="true" className="flex h-full flex-col gap-2 p-4">
      <div className="bg-bg-secondary h-4 w-3/4 animate-pulse rounded" />
      <div className="bg-bg-secondary h-4 w-1/2 animate-pulse rounded" />
      <div className="bg-bg-secondary mt-2 h-24 w-full animate-pulse rounded" />
    </div>
  )
}

function extractReplyAddress(sender: string): string {
  const match = sender.match(/<([^>]+)>/)
  if (match) return match[1].trim()
  return sender.trim()
}

function initialSubject(
  draft: ComposeDraftSource | null,
  reply: ComposeReplySource | null
): string {
  if (draft) return draft.subject
  if (reply) return withReplyPrefix(reply.subject)
  return ''
}

function initialTo(draft: ComposeDraftSource | null, reply: ComposeReplySource | null): string {
  if (draft) return draft.to
  if (reply) return extractReplyAddress(reply.sender)
  return ''
}

function withReplyPrefix(subject: string): string {
  const trimmed = subject.trim()
  if (/^re:/i.test(trimmed)) return trimmed
  return `Re: ${trimmed}`
}
