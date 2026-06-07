import type { TemplateInfo } from './types'
import type { StructuredComposeHandle } from '@/components/structured-compose'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useAtomValue, useSetAtom } from 'jotai'
import { X } from 'lucide-react'
import { Suspense, useEffect, useRef, useState } from 'react'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { formatFullDate } from '@/lib/format'
import { escapeHtml } from '@/lib/html-utils'
import { lazyWithReload } from '@/lib/lazy'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { authAtom, getToken } from '@/store/auth'
import { composeReplySourceAtom, composingNewAtom } from '@/store/chat'
import { signatureAtom, signatureEnabledAtom } from '@/store/settings'

import { ActionBar } from './action-bar'
import { AddressFields } from './address-fields'

// Defer TipTap until the user actually opens the composer. Saves ~700 kB
// off the chat-list / dashboard chunk for users who never compose.
const StructuredCompose = lazyWithReload(() =>
  import('@/components/structured-compose').then((m) => ({ default: m.StructuredCompose }))
)

type PolishResult = { message?: string; polished?: string; success: boolean }
type SendResult = { message?: string; message_id?: string; success: boolean }

export function NewConversation() {
  const auth = useAtomValue(authAtom)
  const signature = useAtomValue(signatureAtom)
  const signatureEnabled = useAtomValue(signatureEnabledAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const replySource = useAtomValue(composeReplySourceAtom)
  const setReplySource = useSetAtom(composeReplySourceAtom)
  const isReply = replySource !== null

  const [to, setTo] = useState(() => (replySource ? extractReplyAddress(replySource.sender) : ''))
  const [cc, setCc] = useState('')
  const [bcc, setBcc] = useState('')
  const [showCcBcc, setShowCcBcc] = useState(false)
  const [subject, setSubject] = useState(() =>
    replySource ? withReplyPrefix(replySource.subject) : ''
  )
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
    queryFn: () => fetchJson<TemplateInfo[]>('/mail/templates'),
  })

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
      const result = await postJson<PolishResult>('/mail/ai/polish', { text })
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
      const result = await postJson<{
        message?: string
        subject?: string
        success: boolean
      }>('/mail/ai/generate-subject', {
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
    const recipients = to
      .split(/[,;]/)
      .map((s) => s.trim())
      .filter(Boolean)
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
      const ccList = cc
        .split(/[,;]/)
        .map((s) => s.trim())
        .filter(Boolean)
      const bccList = bcc
        .split(/[,;]/)
        .map((s) => s.trim())
        .filter(Boolean)

      const attachmentFiles = content.attachments
      let result: SendResult

      if (attachmentFiles.length > 0) {
        const formData = new FormData()
        formData.append('from', auth?.address ?? '')
        formData.append('subject', subject)
        formData.append('body', content.fullText)
        formData.append('html_body', content.fullHtml)
        for (const r of recipients) formData.append('to', r)
        for (const r of ccList) formData.append('cc', r)
        for (const r of bccList) formData.append('bcc', r)
        for (const f of attachmentFiles) formData.append('attachments', f)
        if (replySource?.messageId) formData.append('in_reply_to', replySource.messageId)
        if (scheduledAt) formData.append('scheduled_at', new Date(scheduledAt).toISOString())

        const token = getToken()
        const res = await fetch('/api/mail/send-multipart', {
          body: formData,
          headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}) },
          method: 'POST',
        })
        if (!res.ok) {
          setError(`Send failed (${res.status})`)
          setSending(false)
          return
        }
        result = await res.json()
      } else {
        result = await postJson<SendResult>('/mail/send', {
          bcc: bccList,
          body: content.fullText,
          cc: ccList,
          from: auth?.address ?? '',
          html_body: content.fullHtml,
          in_reply_to: replySource?.messageId ?? null,
          subject,
          to: recipients,
          ...(scheduledAt ? { scheduled_at: new Date(scheduledAt).toISOString() } : {}),
        })
      }

      if (result.success) {
        const sentMessageId = result.message_id
        toast.success('Message sent', {
          ...(sentMessageId
            ? {
                action: {
                  label: 'Undo',
                  onClick: async () => {
                    try {
                      await deleteJson(`/mail/pending/${encodeURIComponent(sentMessageId)}`)
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
        // refresh the conversation list via RQ so the just-sent thread
        // appears at the top.
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

// pull the bare address out of a "Name <addr@host>" sender field, falling
// back to the raw string if no angle-bracketed form is present
function extractReplyAddress(sender: string): string {
  const match = sender.match(/<([^>]+)>/)
  return match ? match[1].trim() : sender.trim()
}

function withReplyPrefix(subject: string): string {
  const trimmed = subject.trim()
  return /^re:/i.test(trimmed) ? trimmed : `Re: ${trimmed}`
}
