import type { ReplyMode } from './types'
import type { StructuredComposeHandle } from '@/components/structured-compose'

import { toast } from '@goliapkg/gds'
import { useAtomValue } from 'jotai'
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { useCurrentThreadMessages } from '@/hooks/use-current-mail-filters'
import { useDeleteDraftMutation, useDraftsQuery, useSaveDraftMutation } from '@/hooks/use-drafts'
import { buildForwardHeaderHtml, escapeHtml } from '@/lib/html-utils'
import { parseAddressList, sendMail } from '@/lib/send-mail'
import { authAtom } from '@/store/auth'
import { signatureAtom, signatureEnabledAtom } from '@/store/settings'
import { wirePolishText, wireReplySuggest } from '@/wire/endpoints/ai'
import { wireDeletePendingSend } from '@/wire/endpoints/mail'

const StructuredCompose = lazy(() =>
  import('@/components/structured-compose').then((m) => ({ default: m.StructuredCompose }))
)

import { ActionBar } from './action-bar'
import { ModeToggle } from './mode-toggle'
import { PreviewDialog } from './preview-dialog'
import { SuggestionsRow } from './suggestions-row'

type ReplyBoxProps = {
  forwardAttachmentsUid?: null | number
  forwardMessageId?: null | string
  lastMessageId: string
  mode: ReplyMode
  onModeChange: (mode: ReplyMode) => void
  onSent: () => void
  originalBody: string
  originalDate: string
  originalFrom: string
  originalHtmlBody?: null | string
  replyAllRecipients: string
  replyRecipients: string
  subject: string
  threadId: string
}
export function ReplyBox({
  forwardAttachmentsUid,
  forwardMessageId,
  lastMessageId,
  mode,
  onModeChange,
  onSent,
  originalBody,
  originalDate,
  originalFrom,
  originalHtmlBody,
  replyAllRecipients,
  replyRecipients,
  subject,
  threadId,
}: ReplyBoxProps) {
  const auth = useAtomValue(authAtom)
  const signature = useAtomValue(signatureAtom)
  const signatureEnabled = useAtomValue(signatureEnabledAtom)
  // v2.1 phase-5d: read thread messages from RQ but pin the value into
  // a ref so the suggest() callback reads the latest without subscribing
  // the whole component to every WS refetch of the open thread (which
  // would re-render TipTap on every arrive/change).
  const threadMessages = useCurrentThreadMessages()
  const threadMessagesRef = useRef(threadMessages)
  threadMessagesRef.current = threadMessages
  const [forwardTo, setForwardTo] = useState('')
  const [sending, setSending] = useState(false)
  // MRS-15: preview-before-send. After the broken-list incident where
  // TipTap's getHTML() output looked clean in the compose pane but wrapped
  // the signature into a bullet list when rendered by the recipient, we
  // surface the actual to-send HTML in a sandboxed iframe.
  const [previewHtml, setPreviewHtml] = useState<null | string>(null)
  // refs to avoid stale closures in send callback
  const forwardMessageIdRef = useRef(forwardMessageId)
  forwardMessageIdRef.current = forwardMessageId
  const forwardAttachmentsUidRef = useRef(forwardAttachmentsUid)
  forwardAttachmentsUidRef.current = forwardAttachmentsUid
  const modeRef = useRef(mode)
  modeRef.current = mode
  const [polishing, setPolishing] = useState(false)
  const [suggesting, setSuggesting] = useState(false)
  const [suggestions, setSuggestions] = useState<string[]>([])
  const [polishTone, setPolishTone] = useState('professional')
  const [error, setError] = useState('')
  const composeRef = useRef<StructuredComposeHandle>(null)
  const prePolishRef = useRef<null | string>(null)

  // server-backed autosave (2026-07-17): the inline reply used to keep
  // its draft in localStorage only, so it never appeared in the Draft
  // tab and didn't survive across browsers. Same upsert dance as the
  // full-screen composer: first save allocates an id, later saves reuse
  // it, send deletes it.
  const draftIdRef = useRef<null | number>(null)
  const sentRef = useRef(false)
  const restoredRef = useRef(false)
  const lastSavedRef = useRef('')
  const saveDraftMut = useSaveDraftMutation()
  const deleteDraftMut = useDeleteDraftMutation()
  const { data: serverDrafts = [] } = useDraftsQuery()

  const recipientsRef = useRef({ forwardTo, mode, replyAllRecipients, replyRecipients })
  recipientsRef.current = { forwardTo, mode, replyAllRecipients, replyRecipients }

  const currentTo = useCallback(() => {
    const r = recipientsRef.current
    switch (r.mode) {
      case 'forward':
        return r.forwardTo
      case 'reply-all':
        return r.replyAllRecipients
      default:
        return r.replyRecipients
    }
  }, [])

  const saveDraftServer = useCallback(async () => {
    if (sentRef.current) return
    const body = composeRef.current?.getMarkdown() ?? ''
    if (!body.trim()) return
    const snapshot = body
    if (snapshot === lastSavedRef.current) return
    try {
      const res = await saveDraftMut.mutateAsync({
        body,
        id: draftIdRef.current ?? undefined,
        reply_to_thread_id: threadId,
        subject,
        to: currentTo(),
      })
      if (res.id !== undefined) draftIdRef.current = Number(res.id)
      lastSavedRef.current = snapshot
    } catch {
      // transient — next tick retries
    }
  }, [saveDraftMut, threadId, subject, currentTo])

  // restore this thread's reply draft once the server list arrives
  useEffect(() => {
    if (restoredRef.current || serverDrafts.length === 0) return
    const mine = serverDrafts.find((d) => d.reply_to_thread_id === threadId)
    if (!mine || !mine.body.trim()) return
    restoredRef.current = true
    draftIdRef.current = Number(mine.id)
    lastSavedRef.current = mine.body
    setTimeout(() => {
      const handle = composeRef.current
      if (handle && !handle.getMarkdown().trim()) {
        handle.setMarkdown(mine.body)
        toast.info('Draft restored', { duration: 2000 })
      }
    }, 200)
  }, [serverDrafts, threadId])

  // periodic save while typing — bind the interval ONCE; the latest
  // closure is read through a ref.
  const saveDraftServerRef = useRef(saveDraftServer)
  saveDraftServerRef.current = saveDraftServer
  useEffect(() => {
    const timer = setInterval(() => void saveDraftServerRef.current(), 3000)
    return () => clearInterval(timer)
  }, [])

  const handleModeChange = (newMode: ReplyMode) => {
    onModeChange(newMode)
    setSuggestions([])
    setError('')
  }

  const resolveRecipients = (): string[] => {
    if (mode === 'reply') return parseAddressList(replyRecipients)
    if (mode === 'reply-all') return parseAddressList(replyAllRecipients)
    return parseAddressList(forwardTo)
  }

  const resolveSubject = (): string => {
    if (mode === 'forward') return subject.startsWith('Fwd:') ? subject : `Fwd: ${subject}`
    return subject.startsWith('Re:') ? subject : `Re: ${subject}`
  }

  const send = async () => {
    if (sending) return

    const to = resolveRecipients()
    if (modeRef.current === 'forward' && to.length === 0) {
      setError('Please enter at least one recipient')
      return
    }

    const content = composeRef.current?.getContent()
    if (!content || !content.compose.text.trim()) {
      toast.error('Message body is required')
      return
    }

    setError('')
    setSending(true)

    const resolvedSubject = resolveSubject()
    const currentMode = modeRef.current
    const inReplyTo = currentMode === 'forward' ? undefined : lastMessageId

    try {
      const fwdMid = currentMode === 'forward' ? forwardMessageIdRef.current : null
      const fwdUid = currentMode === 'forward' ? forwardAttachmentsUidRef.current : null
      // when forwarding via backend (forward_message_id) AND there are no
      // user-added attachments, the body field should carry only the user's
      // typed text — backend appends the original body / attachments from
      // the raw .eml. With attachments we use fullText/fullHtml as normal.
      const isBackendForward =
        currentMode === 'forward' && (fwdMid || fwdUid) && content.attachments.length === 0
      const body = isBackendForward ? content.compose.text : content.fullText
      const htmlBody = isBackendForward ? content.compose.html : content.fullHtml

      const result = await sendMail({
        attachments: content.attachments,
        body,
        forwardAttachmentsFrom: fwdUid && fwdUid > 0 ? fwdUid : undefined,
        forwardMessageId: fwdMid && fwdMid.length > 0 ? fwdMid : undefined,
        from: auth?.address ?? '',
        htmlBody,
        inReplyTo,
        subject: resolvedSubject,
        to,
        token: auth?.token ?? '',
      })

      if (!result.success) {
        toast.error(result.message ?? 'Send failed')
        return
      }
      const sentMessageId = result.message_id

      composeRef.current?.clearCompose()
      setForwardTo('')
      const label = currentMode === 'forward' ? 'Forwarded' : 'Reply sent'
      toast.success(label, {
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
      sentRef.current = true
      if (draftIdRef.current !== null) {
        deleteDraftMut.mutate(draftIdRef.current)
        draftIdRef.current = null
      }
      onSent()
    } catch (err) {
      void saveDraftServer()
      toast.error(err instanceof Error ? err.message : 'Network error — draft saved', {
        action: { label: 'Retry', onClick: () => send() },
        duration: 10000,
      })
    } finally {
      setSending(false)
    }
  }

  const polish = async (tone?: string) => {
    const handle = composeRef.current
    if (!handle) return
    const text = handle.getMarkdown()
    if (!text.trim()) return
    prePolishRef.current = text
    setPolishing(true)
    try {
      const result = await wirePolishText(text, tone ?? polishTone)
      if (result.success && result.polished) {
        handle.setMarkdown(result.polished)
        toast.success('Text polished', {
          action: {
            label: 'Undo',
            onClick: () => {
              if (prePolishRef.current != null) {
                handle.setMarkdown(prePolishRef.current)
                prePolishRef.current = null
              }
            },
          },
          duration: 8000,
        })
      } else {
        toast.error(result.message ?? 'Polish failed')
        prePolishRef.current = null
      }
    } catch {
      toast.error('AI unavailable')
      prePolishRef.current = null
    } finally {
      setPolishing(false)
    }
  }

  const suggest = async () => {
    setSuggesting(true)
    try {
      // build thread context from prior messages (up to 3, excluding the latest)
      const priorMessages = threadMessagesRef.current.slice(0, -1).slice(-3)
      const threadContext =
        priorMessages.length > 0
          ? priorMessages
              .map((m) => `From: ${m.sender}\n${m.clean_text || m.text_body || ''}`)
              .join('\n---\n')
          : undefined
      const result = await wireReplySuggest({
        original_body: originalBody,
        sender: originalFrom,
        subject,
        thread_context: threadContext,
      })
      if (result.success && result.suggestions.length > 0) {
        setSuggestions(result.suggestions)
      } else {
        toast.error(result.message ?? 'No suggestions')
      }
    } catch {
      toast.error('AI unavailable')
    } finally {
      setSuggesting(false)
    }
  }

  const applySuggestion = (suggestion: string) => {
    composeRef.current?.setMarkdown(suggestion)
    setSuggestions([])
    toast.success('Suggestion applied')
  }

  const openPreview = () => {
    const content = composeRef.current?.getContent()
    setPreviewHtml(content?.fullHtml ?? '')
  }

  const quotedHtml =
    mode === 'forward' && originalHtmlBody ? originalHtmlBody : originalBody || undefined
  const quotedHeaderHtml =
    mode === 'forward'
      ? buildForwardHeaderHtml(originalFrom, originalDate, subject)
      : originalFrom
        ? `<p style="color:#888">On ${escapeHtml(originalDate)}, ${escapeHtml(originalFrom)} wrote:</p>`
        : undefined
  const quotedHeader =
    mode === 'forward'
      ? `---------- Forwarded message ----------\nFrom: ${originalFrom}\nDate: ${originalDate}\nSubject: ${subject}\n\n`
      : originalFrom
        ? `On ${originalDate}, ${originalFrom} wrote:\n\n`
        : ''

  const placeholder = mode === 'forward' ? 'Add a message...' : 'Type a reply...'

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ModeToggle
        mode={mode}
        onChange={handleModeChange}
        replyAllRecipients={replyAllRecipients}
        replyRecipients={replyRecipients}
      />

      {mode === 'forward' && (
        <div className="border-border shrink-0 border-b px-4 py-2">
          <div className="flex items-center gap-2">
            <label className="text-fg-muted w-16 shrink-0 text-xs" htmlFor="forward-to-input">
              To
            </label>
            <ContactAutocomplete
              className="border-border text-fg placeholder-fg-muted focus:border-accent w-full rounded-md border bg-transparent px-2 py-1 text-sm outline-none"
              onChange={setForwardTo}
              placeholder="recipient@example.com"
              value={forwardTo}
            />
          </div>
          {error && (
            <p className="text-danger mt-1 text-xs" role="alert">
              {error}
            </p>
          )}
        </div>
      )}

      <SuggestionsRow
        onApply={applySuggestion}
        onDismiss={() => setSuggestions([])}
        suggestions={suggestions}
      />

      {/* block-based composer */}
      <div className="min-h-0 flex-1 overflow-hidden">
        <Suspense fallback={<ComposerSkeleton />}>
          <StructuredCompose
            disabled={sending}
            mode={mode === 'forward' ? 'forward' : 'reply'}
            onSubmit={send}
            placeholder={placeholder}
            quotedHeader={quotedHeader}
            quotedHeaderHtml={quotedHeaderHtml}
            quotedHtml={quotedHtml}
            ref={composeRef}
            signature={signature}
            signatureEnabled={signatureEnabled}
          />
        </Suspense>
      </div>

      <ActionBar
        mode={mode}
        onPolish={() => void polish()}
        onPreview={openPreview}
        onSend={() => void send()}
        onSuggest={() => void suggest()}
        onToneChange={setPolishTone}
        polishing={polishing}
        sending={sending}
        suggesting={suggesting}
        tone={polishTone}
      />

      {previewHtml !== null && (
        <PreviewDialog
          html={previewHtml}
          onClose={() => setPreviewHtml(null)}
          onSend={() => {
            setPreviewHtml(null)
            void send()
          }}
          sending={sending}
        />
      )}
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
