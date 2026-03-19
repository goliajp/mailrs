import { useAtomValue } from 'jotai'
import { Loader2, Send } from 'lucide-react'
import { useRef, useState } from 'react'
import { toast } from 'sonner'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { StructuredCompose, type StructuredComposeHandle } from '@/components/structured-compose'
import { deleteJson, postJson } from '@/lib/api'
import { escapeHtml, buildForwardHeaderHtml } from '@/lib/html-utils'
import { authAtom } from '@/store/auth'
import { threadMessagesAtom } from '@/store/chat'
import { signatureAtom, signatureEnabledAtom } from '@/store/settings'

export type ReplyMode = 'reply' | 'reply-all' | 'forward'

type SendResult = { success: boolean; message?: string; message_id?: string }
type ReplySuggestResult = { success: boolean; suggestions: string[]; message?: string }
type PolishResult = { success: boolean; polished?: string; message?: string }

const MODE_LABELS: Record<ReplyMode, string> = {
  reply: 'Reply',
  'reply-all': 'Reply All',
  forward: 'Forward',
}

export function ReplyBox({
  lastMessageId,
  replyRecipients,
  replyAllRecipients,
  subject,
  originalFrom,
  originalDate,
  originalBody,
  onSent,
  mode,
  onModeChange,
}: {
  threadId: string
  lastMessageId: string
  replyRecipients: string
  replyAllRecipients: string
  subject: string
  originalFrom: string
  originalDate: string
  originalBody: string
  onSent: () => void
  mode: ReplyMode
  onModeChange: (mode: ReplyMode) => void
}) {
  const auth = useAtomValue(authAtom)
  const signature = useAtomValue(signatureAtom)
  const signatureEnabled = useAtomValue(signatureEnabledAtom)
  const threadMessages = useAtomValue(threadMessagesAtom)
  const [forwardTo, setForwardTo] = useState('')
  const [sending, setSending] = useState(false)
  const [polishing, setPolishing] = useState(false)
  const [suggesting, setSuggesting] = useState(false)
  const [suggestions, setSuggestions] = useState<string[]>([])
  const [error, setError] = useState('')
  const composeRef = useRef<StructuredComposeHandle>(null)

  const handleModeChange = (newMode: ReplyMode) => {
    onModeChange(newMode)
    setSuggestions([])
    setError('')
  }

  const resolveRecipients = (): string[] => {
    if (mode === 'reply') return replyRecipients.split(',').map((s) => s.trim()).filter(Boolean)
    if (mode === 'reply-all') return replyAllRecipients.split(',').map((s) => s.trim()).filter(Boolean)
    return forwardTo.split(',').map((s) => s.trim()).filter(Boolean)
  }

  const resolveSubject = (): string => {
    if (mode === 'forward') return subject.startsWith('Fwd:') ? subject : `Fwd: ${subject}`
    return subject.startsWith('Re:') ? subject : `Re: ${subject}`
  }

  const send = async () => {
    if (sending) return

    const to = resolveRecipients()
    if (mode === 'forward' && to.length === 0) {
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
    const inReplyTo = mode === 'forward' ? undefined : lastMessageId

    try {
      let sentMessageId: string | undefined
      const assembled = content

      const attachmentFiles = assembled.attachments
      const hasAttachments = attachmentFiles.length > 0

      if (hasAttachments) {
        const formData = new FormData()
        formData.append('from', auth?.address ?? '')
        formData.append('subject', resolvedSubject)
        formData.append('body', assembled.fullText)
        formData.append('html_body', assembled.fullHtml)
        if (inReplyTo) formData.append('in_reply_to', inReplyTo)
        for (const r of to) formData.append('to', r)
        for (const f of attachmentFiles) formData.append('attachments', f)

        const res = await fetch('/api/mail/send-multipart', {
          method: 'POST',
          headers: { Authorization: `Bearer ${auth?.token ?? ''}` },
          body: formData,
        })
        if (!res.ok) { toast.error(`Send failed (${res.status})`); return }
        const result: SendResult = await res.json()
        if (!result.success) {
          toast.error(result.message ?? 'Send failed')
          return
        }
        sentMessageId = result.message_id
      } else {
        const payload: Record<string, unknown> = {
          from: auth?.address ?? '',
          to,
          cc: [],
          bcc: [],
          subject: resolvedSubject,
          body: assembled.fullText,
          html_body: assembled.fullHtml,
        }
        if (inReplyTo) payload['in_reply_to'] = inReplyTo

        const result = await postJson<SendResult>('/mail/send', payload)
        if (!result.success) {
          toast.error(result.message ?? 'Send failed')
          return
        }
        sentMessageId = result.message_id
      }

      composeRef.current?.clearCompose()
      setForwardTo('')
      const label = mode === 'forward' ? 'Forwarded' : 'Reply sent'
      toast.success(label, {
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
      onSent()
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Network error')
    } finally {
      setSending(false)
    }
  }

  const prePolishRef = useRef<string | null>(null)

  const [polishTone, setPolishTone] = useState('professional')

  const polish = async (tone?: string) => {
    const editor = composeRef.current?.getComposeEditor()
    if (!editor) return
    const text = editor.getText()
    if (!text.trim()) return
    prePolishRef.current = editor.getHTML()
    setPolishing(true)
    try {
      const result = await postJson<PolishResult>('/mail/ai/polish', { text, tone: tone ?? polishTone })
      if (result.success && result.polished) {
        const paragraphs = result.polished.split(/\n+/).filter(Boolean).map((p) => `<p>${escapeHtml(p)}</p>`).join('')
        editor.commands.setContent(paragraphs || `<p>${escapeHtml(result.polished)}</p>`)
        toast.success('Text polished', {
          action: {
            label: 'Undo',
            onClick: () => {
              if (prePolishRef.current) {
                editor.commands.setContent(prePolishRef.current)
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
      const priorMessages = threadMessages.slice(0, -1).slice(-3)
      const threadContext = priorMessages.length > 0
        ? priorMessages.map((m) => `From: ${m.sender}\n${m.clean_text || m.text_body || ''}`).join('\n---\n')
        : undefined
      const result = await postJson<ReplySuggestResult>('/mail/ai/reply-suggest', {
        original_sender: originalFrom,
        original_subject: subject,
        original_body: originalBody,
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
    const paragraphs = suggestion.split(/\n+/).filter(Boolean).map((p) => `<p>${escapeHtml(p)}</p>`).join('')
    composeRef.current?.setComposeContent(paragraphs || `<p>${escapeHtml(suggestion)}</p>`)
    setSuggestions([])
    toast.success('Suggestion applied')
  }

  const quotedHtml = originalBody || undefined
  const quotedHeaderHtml = mode === 'forward'
    ? buildForwardHeaderHtml(originalFrom, originalDate, subject)
    : originalFrom
      ? `<p style="color:#888">On ${escapeHtml(originalDate)}, ${escapeHtml(originalFrom)} wrote:</p>`
      : undefined
  const quotedHeader = mode === 'forward'
    ? `---------- Forwarded message ----------\nFrom: ${originalFrom}\nDate: ${originalDate}\nSubject: ${subject}\n\n`
    : originalFrom ? `On ${originalDate}, ${originalFrom} wrote:\n\n` : ''

  const placeholder = mode === 'forward' ? 'Add a message...' : 'Type a reply...'

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* mode toggle + recipients */}
      <div className="flex shrink-0 select-none items-center gap-1 border-b border-[var(--color-border-default)] px-4 py-2">
        {(Object.keys(MODE_LABELS) as ReplyMode[]).map((m) => (
          <button
            key={m}
            onClick={() => handleModeChange(m)}
            aria-pressed={mode === m}
            className={`cursor-pointer rounded px-2.5 py-1 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:ring-[var(--color-brand-primary)] focus-visible:outline-none ${
              mode === m
                ? 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'
                : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]'
            }`}
          >
            {MODE_LABELS[m]}
          </button>
        ))}
        {mode !== 'forward' && (
          <span className="ml-auto truncate text-xs text-[var(--color-text-tertiary)]" title={mode === 'reply' ? replyRecipients : replyAllRecipients}>
            to {mode === 'reply' ? replyRecipients : replyAllRecipients}
          </span>
        )}
      </div>

      {/* forward: to field */}
      {mode === 'forward' && (
        <div className="shrink-0 border-b border-[var(--color-border-default)] px-4 py-2">
          <div className="flex items-center gap-2">
            <label className="w-16 shrink-0 text-xs text-[var(--color-text-tertiary)]">To</label>
            <ContactAutocomplete
              value={forwardTo}
              onChange={setForwardTo}
              placeholder="recipient@example.com"
              className="w-full rounded-md border border-[var(--color-border-default)] bg-transparent px-2 py-1 text-sm text-[var(--color-text-primary)] placeholder-[var(--color-text-tertiary)] outline-none focus:border-[var(--color-brand-primary)]"
            />
          </div>
          {error && <p className="mt-1 text-xs text-[var(--color-status-danger)]">{error}</p>}
        </div>
      )}

      {/* AI suggestions */}
      {suggestions.length > 0 && (
        <div className="flex shrink-0 flex-wrap gap-1.5 border-b border-[var(--color-border-default)] px-4 py-2">
          {suggestions.map((s, i) => (
            <button key={i} onClick={() => applySuggestion(s)}
              className="max-w-xs truncate rounded-full border border-[var(--color-border-default)] bg-[var(--color-brand-subtle)] px-2.5 py-0.5 text-xs text-[var(--color-brand-primary)] transition-colors hover:bg-[var(--color-hover)]"
              title={s}>{s}</button>
          ))}
          <button onClick={() => setSuggestions([])}
            className="rounded-full px-2 py-0.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]">
            Dismiss
          </button>
        </div>
      )}

      {/* block-based composer */}
      <div className="min-h-0 flex-1 overflow-hidden">
        <StructuredCompose
          ref={composeRef}
          onSubmit={send}
          placeholder={placeholder}
          disabled={sending}
          signature={signature}
          signatureEnabled={signatureEnabled}
          quotedHtml={quotedHtml}
          quotedHeader={quotedHeader}
          quotedHeaderHtml={quotedHeaderHtml}
          mode={mode === 'forward' ? 'forward' : 'reply'}
        />
      </div>

      {/* action bar */}
      <div className="flex shrink-0 select-none flex-wrap items-center gap-1 border-t border-[var(--color-border-default)] px-4 py-2">
        {mode !== 'forward' && (
          <button onClick={suggest} disabled={suggesting || sending}
            className="flex h-8 shrink-0 items-center rounded-md px-2 text-xs text-[var(--color-brand-primary)] transition-colors hover:bg-[var(--color-brand-subtle)] disabled:cursor-not-allowed disabled:text-[var(--color-text-tertiary)] disabled:opacity-50"
            title="AI reply suggestions">
            {suggesting ? <Loader2 className="h-3 w-3 animate-spin" /> : 'Suggest'}
          </button>
        )}
        <div className="relative flex shrink-0">
          <button onClick={() => polish()} disabled={polishing || sending}
            className="flex h-8 items-center rounded-l-md px-2 text-xs text-[var(--color-brand-primary)] transition-colors hover:bg-[var(--color-brand-subtle)] disabled:cursor-not-allowed disabled:text-[var(--color-text-tertiary)] disabled:opacity-50"
            title={`Polish (${polishTone})`}>
            {polishing ? <Loader2 className="h-3 w-3 animate-spin" /> : 'Polish'}
          </button>
          <select
            value={polishTone}
            onChange={(e) => { setPolishTone(e.target.value); polish(e.target.value) }}
            disabled={polishing || sending}
            className="h-8 appearance-none rounded-r-md border-l border-[var(--color-border-default)] bg-transparent px-1 text-[10px] text-[var(--color-brand-primary)] outline-none hover:bg-[var(--color-brand-subtle)] disabled:cursor-not-allowed disabled:opacity-50"
          >
            <option value="professional">Pro</option>
            <option value="casual">Casual</option>
            <option value="formal">Formal</option>
            <option value="friendly">Friendly</option>
            <option value="concise">Concise</option>
          </select>
        </div>
        <div className="flex-1" />

        <button onClick={send} disabled={sending}
          className="flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-[var(--color-brand-primary)] px-3 text-xs font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:cursor-not-allowed disabled:opacity-50">
          <Send className="h-3.5 w-3.5" />
          {sending ? 'Sending…' : 'Send'}
        </button>
      </div>
    </div>
  )
}
