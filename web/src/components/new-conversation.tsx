import type { ConversationSummary } from '@/lib/types'

import { useAtomValue, useSetAtom } from 'jotai'
import { Loader2, Send, Sparkles, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import {
  StructuredCompose,
  type StructuredComposeHandle,
} from '@/components/structured-compose'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { escapeHtml } from '@/lib/html-utils'
import { getToken } from '@/store/auth'
import { authAtom } from '@/store/auth'
import {
  composingNewAtom,
  conversationsAtom,
  selectedThreadIdAtom,
} from '@/store/chat'
import { signatureAtom, signatureEnabledAtom } from '@/store/settings'

type PolishResult = { message?: string; polished?: string; success: boolean }
type SendResult = { message?: string; message_id?: string; success: boolean }
type TemplateInfo = {
  category: string
  html_body: string
  id: number
  name: string
  subject: string
  text_body: string
}

export function NewConversation() {
  const auth = useAtomValue(authAtom)
  const signature = useAtomValue(signatureAtom)
  const signatureEnabled = useAtomValue(signatureEnabledAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setSelectedThread = useSetAtom(selectedThreadIdAtom)
  const setConversations = useSetAtom(conversationsAtom)

  const [to, setTo] = useState('')
  const [cc, setCc] = useState('')
  const [bcc, setBcc] = useState('')
  const [showCcBcc, setShowCcBcc] = useState(false)
  const [subject, setSubject] = useState('')
  const [sending, setSending] = useState(false)
  const [polishing, setPolishing] = useState(false)
  const [generatingSubject, setGeneratingSubject] = useState(false)
  const [error, setError] = useState('')
  const [templates, setTemplates] = useState<TemplateInfo[]>([])
  const [scheduledAt, setScheduledAt] = useState('')
  const [showSchedulePicker, setShowSchedulePicker] = useState(false)
  const composeRef = useRef<StructuredComposeHandle>(null)

  useEffect(() => {
    fetchJson<TemplateInfo[]>('/mail/templates')
      .then(setTemplates)
      .catch(() => {})
  }, [])

  const applyTemplate = (t: TemplateInfo) => {
    setSubject(t.subject)
    if (t.html_body) composeRef.current?.setComposeContent(t.html_body)
  }

  const polish = async () => {
    const editor = composeRef.current?.getComposeEditor()
    if (!editor) return
    const text = editor.getText()
    if (!text.trim()) return
    setPolishing(true)
    try {
      const result = await postJson<PolishResult>('/mail/ai/polish', { text })
      if (result.success && result.polished) {
        const paragraphs = result.polished
          .split(/\n+/)
          .filter(Boolean)
          .map((p) => `<p>${escapeHtml(p)}</p>`)
          .join('')
        editor.commands.setContent(
          paragraphs || `<p>${escapeHtml(result.polished)}</p>`
        )
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
        if (scheduledAt)
          formData.append('scheduled_at', new Date(scheduledAt).toISOString())

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
          in_reply_to: null,
          subject,
          to: recipients,
          ...(scheduledAt
            ? { scheduled_at: new Date(scheduledAt).toISOString() }
            : {}),
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
                      await deleteJson(
                        `/mail/pending/${encodeURIComponent(sentMessageId)}`
                      )
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
        const convos = await fetchJson<ConversationSummary[]>(
          '/conversations?limit=50'
        )
        setConversations(convos)
        if (convos.length > 0) setSelectedThread(convos[0].thread_id)
        setComposingNew(false)
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
      <div className="flex shrink-0 items-center justify-between border-b border-[var(--color-border-default)] px-4 py-2">
        <span className="text-xs font-medium text-[var(--color-text-tertiary)]">
          New message
        </span>
        <button
          aria-label="Close"
          className="rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
          onClick={() => setComposingNew(false)}
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {error && (
        <div className="mx-4 mt-2 rounded-md bg-[var(--color-status-danger-subtle)] px-3 py-1.5 text-xs text-[var(--color-status-danger)]">
          {error}
        </div>
      )}

      {/* address fields — consistent label width for alignment */}
      <div className="flex shrink-0 flex-col border-b border-[var(--color-border-default)]">
        <div className="flex h-9 items-center border-b border-[var(--color-border-default)] px-4">
          <label className="w-14 shrink-0 text-xs text-[var(--color-text-tertiary)]">
            To
          </label>
          <ContactAutocomplete
            autoFocus
            onChange={setTo}
            placeholder="recipient@example.com"
            value={to}
          />
          {!showCcBcc && (
            <button
              className="shrink-0 text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
              onClick={() => setShowCcBcc(true)}
            >
              Cc/Bcc
            </button>
          )}
        </div>
        {showCcBcc && (
          <>
            <div className="flex h-9 items-center border-b border-[var(--color-border-default)] px-4">
              <label className="w-14 shrink-0 text-xs text-[var(--color-text-tertiary)]">
                Cc
              </label>
              <ContactAutocomplete
                onChange={setCc}
                placeholder="cc@example.com"
                value={cc}
              />
            </div>
            <div className="flex h-9 items-center border-b border-[var(--color-border-default)] px-4">
              <label className="w-14 shrink-0 text-xs text-[var(--color-text-tertiary)]">
                Bcc
              </label>
              <ContactAutocomplete
                onChange={setBcc}
                placeholder="bcc@example.com"
                value={bcc}
              />
            </div>
          </>
        )}
        <div className="flex h-9 items-center border-b border-[var(--color-border-default)] px-4">
          <label
            className="w-14 shrink-0 text-xs text-[var(--color-text-tertiary)]"
            htmlFor="new-conv-subject"
          >
            Subject
          </label>
          <input
            className="flex-1 bg-transparent py-2 text-sm text-[var(--color-text-primary)] outline-none"
            id="new-conv-subject"
            onChange={(e) => setSubject(e.target.value)}
            type="text"
            value={subject}
          />
          <button
            className="shrink-0 rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-brand-subtle)] hover:text-[var(--color-brand-primary)] disabled:cursor-not-allowed disabled:opacity-50"
            disabled={generatingSubject || sending}
            onClick={generateSubject}
            title="AI generate subject"
            type="button"
          >
            {generatingSubject ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Sparkles className="h-3.5 w-3.5" />
            )}
          </button>
        </div>
      </div>

      {/* block-based composer */}
      <div className="min-h-0 flex-1">
        <StructuredCompose
          disabled={sending}
          mode="new"
          onSubmit={send}
          placeholder="Write your message..."
          ref={composeRef}
          signature={signature}
          signatureEnabled={signatureEnabled}
        />
      </div>

      {/* action bar */}
      <div className="flex shrink-0 flex-wrap items-center gap-1 border-t border-[var(--color-border-default)] px-4 py-2 select-none">
        {scheduledAt && (
          <span className="inline-flex items-center gap-1 rounded-full bg-[var(--color-brand-subtle)] px-2.5 py-0.5 text-xs text-[var(--color-brand-primary)]">
            {new Date(scheduledAt).toLocaleString(undefined, {
              day: 'numeric',
              hour: 'numeric',
              minute: '2-digit',
              month: 'short',
            })}
            <button
              aria-label="Clear schedule"
              className="rounded-full p-0.5 hover:opacity-70"
              onClick={() => {
                setScheduledAt('')
                setShowSchedulePicker(false)
              }}
            >
              <X className="h-3 w-3" />
            </button>
          </span>
        )}
        <button
          className="flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-[var(--color-brand-primary)] px-4 text-sm font-medium text-white transition-all hover:bg-[var(--color-brand-primary-hover)] hover:shadow-md active:scale-95 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={sending}
          onClick={send}
        >
          <Send className="h-3.5 w-3.5" />
          {sending ? 'Sending…' : 'Send'}
        </button>

        <button
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${showSchedulePicker ? 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]' : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)]'}`}
          disabled={sending}
          onClick={() => setShowSchedulePicker((v) => !v)}
          title="Schedule send"
        >
          <svg
            className="h-4 w-4"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            viewBox="0 0 24 24"
          >
            <circle cx="12" cy="12" r="10" />
            <path d="M12 6v6l4 2" strokeLinecap="round" />
          </svg>
        </button>
        {showSchedulePicker && (
          <input
            aria-label="Schedule send time"
            className="h-8 w-44 max-w-full shrink-0 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand-primary)]"
            min={(() => {
              const d = new Date()
              return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}T${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
            })()}
            onChange={(e) => setScheduledAt(e.target.value)}
            type="datetime-local"
            value={scheduledAt}
          />
        )}

        <div className="mx-0.5 h-4 w-px bg-[var(--color-border-default)]" />

        <button
          className="flex h-8 shrink-0 items-center rounded-md px-2 text-xs text-[var(--color-brand-primary)] transition-colors hover:bg-[var(--color-brand-subtle)] disabled:cursor-not-allowed disabled:text-[var(--color-text-tertiary)] disabled:opacity-50"
          disabled={polishing || sending}
          onClick={polish}
          title="AI polish"
        >
          {polishing ? <Loader2 className="h-3 w-3 animate-spin" /> : 'Polish'}
        </button>

        {templates.length > 0 && (
          <select
            className="h-8 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 text-xs text-[var(--color-text-secondary)] disabled:cursor-not-allowed disabled:opacity-50"
            defaultValue=""
            disabled={sending}
            onChange={(e) => {
              const t = templates.find((t) => t.id === Number(e.target.value))
              if (t) applyTemplate(t)
              e.target.value = ''
            }}
          >
            <option disabled value="">
              Templates
            </option>
            {templates.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        )}

        <div className="flex-1" />
        <kbd className="mr-1 hidden text-[10px] text-[var(--color-text-tertiary)] select-none sm:inline">
          {typeof navigator !== 'undefined' &&
          /Mac|iPhone|iPad/.test(navigator.userAgent)
            ? '⌘'
            : 'Ctrl+'}
          ↵
        </kbd>
        <button
          className="flex h-8 shrink-0 items-center rounded-md px-3 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] disabled:cursor-not-allowed disabled:opacity-50"
          disabled={sending}
          onClick={() => setComposingNew(false)}
        >
          Cancel
        </button>
      </div>
    </div>
  )
}
