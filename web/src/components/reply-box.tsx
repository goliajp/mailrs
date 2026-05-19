import { toast } from '@goliapkg/gds'
import { useAtomValue, useStore } from 'jotai'
import { Eye, Loader2, Send } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { StructuredCompose, type StructuredComposeHandle } from '@/components/structured-compose'
import { deleteJson, postJson } from '@/lib/api'
import { buildForwardHeaderHtml, escapeHtml } from '@/lib/html-utils'
import { authAtom } from '@/store/auth'
import { threadMessagesAtom } from '@/store/chat'
import { signatureAtom, signatureEnabledAtom } from '@/store/settings'

export type ReplyMode = 'forward' | 'reply' | 'reply-all'

type PolishResult = { message?: string; polished?: string; success: boolean }
type ReplySuggestResult = {
  message?: string
  success: boolean
  suggestions: string[]
}
type SendResult = { message?: string; message_id?: string; success: boolean }

const MODE_LABELS: Record<ReplyMode, string> = {
  forward: 'Forward',
  reply: 'Reply',
  'reply-all': 'Reply All',
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
}: {
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
}) {
  const auth = useAtomValue(authAtom)
  const signature = useAtomValue(signatureAtom)
  const signatureEnabled = useAtomValue(signatureEnabledAtom)
  // Read threadMessages imperatively in the suggest handler — subscribing
  // via useAtomValue re-renders the TipTap editor (heavy) on every WS
  // refetch of the open thread. The data is only used inside an
  // event-handler closure, never during render.
  const store = useStore()
  const [forwardTo, setForwardTo] = useState('')
  const [sending, setSending] = useState(false)
  // MRS-15 follow-up: preview-before-send. After the broken-list incident
  // where TipTap's getHTML() output looked clean in the compose pane but
  // wrapped the signature into a bullet list when rendered by the
  // recipient, we surface the actual to-send HTML in a sandboxed iframe so
  // users can spot WYSIWYG drift before hitting send.
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
  const [error, setError] = useState('')
  const composeRef = useRef<StructuredComposeHandle>(null)

  // auto-save draft to localStorage every 3s
  const draftKey = `mailrs_draft_${lastMessageId || 'new'}`
  const saveTimer = useRef<ReturnType<typeof setTimeout>>(null)

  const saveDraftLocal = useCallback(() => {
    const md = composeRef.current?.getMarkdown() ?? ''
    if (md.trim()) {
      localStorage.setItem(draftKey, md)
    }
  }, [draftKey])

  // restore draft on mount
  useEffect(() => {
    const saved = localStorage.getItem(draftKey)
    if (saved) {
      setTimeout(() => {
        composeRef.current?.setMarkdown(saved)
        toast.info('Draft restored', { duration: 2000 })
      }, 200)
    }
    return () => {
      if (saveTimer.current) clearTimeout(saveTimer.current)
    }
  }, [draftKey])

  // periodic save while typing — bind the interval ONCE; the latest
  // saveDraftLocal closure is read through a ref so we don't tear down
  // and rebuild the interval every time draftKey changes (which happens
  // on every thread switch / replyMode change downstream).
  const saveDraftLocalRef = useRef(saveDraftLocal)
  saveDraftLocalRef.current = saveDraftLocal
  useEffect(() => {
    saveTimer.current = setInterval(() => saveDraftLocalRef.current(), 3000)
    return () => {
      if (saveTimer.current) clearInterval(saveTimer.current)
    }
  }, [])

  const handleModeChange = (newMode: ReplyMode) => {
    onModeChange(newMode)
    setSuggestions([])
    setError('')
  }

  const resolveRecipients = (): string[] => {
    if (mode === 'reply')
      return replyRecipients
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
    if (mode === 'reply-all')
      return replyAllRecipients
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
    return forwardTo
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
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
        if (currentMode === 'forward') {
          const fmid = forwardMessageIdRef.current
          const fuid = forwardAttachmentsUidRef.current
          if (fmid) formData.append('forward_message_id', fmid)
          if (fuid) formData.append('forward_attachments_from', String(fuid))
        }
        for (const r of to) formData.append('to', r)
        for (const f of attachmentFiles) formData.append('attachments', f)

        const res = await fetch('/api/mail/send-multipart', {
          body: formData,
          headers: { Authorization: `Bearer ${auth?.token ?? ''}` },
          method: 'POST',
        })
        if (!res.ok) {
          toast.error(`Send failed (${res.status})`)
          return
        }
        const result: SendResult = await res.json()
        if (!result.success) {
          toast.error(result.message ?? 'Send failed')
          return
        }
        sentMessageId = result.message_id
      } else {
        // when forwarding via backend (forward_message_id), send only the user's
        // typed text — the backend reads the original body + attachments from raw .eml
        const fwdMid = forwardMessageIdRef.current
        const fwdUid = forwardAttachmentsUidRef.current
        const isBackendForward = currentMode === 'forward' && (fwdMid || fwdUid)
        const payload: Record<string, unknown> = {
          bcc: [],
          body: isBackendForward ? assembled.compose.text : assembled.fullText,
          cc: [],
          from: auth?.address ?? '',
          html_body: isBackendForward ? assembled.compose.html : assembled.fullHtml,
          subject: resolvedSubject,
          to,
        }
        if (inReplyTo) payload['in_reply_to'] = inReplyTo
        if (currentMode === 'forward') {
          if (fwdMid && fwdMid.length > 0) payload['forward_message_id'] = fwdMid
          if (fwdUid && fwdUid > 0) payload['forward_attachments_from'] = fwdUid
        }

        const result = await postJson<SendResult>('/mail/send', payload)
        if (!result.success) {
          toast.error(result.message ?? 'Send failed')
          return
        }
        sentMessageId = result.message_id
      }

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
      localStorage.removeItem(draftKey)
      onSent()
    } catch (err) {
      saveDraftLocal()
      toast.error(err instanceof Error ? err.message : 'Network error — draft saved', {
        action: { label: 'Retry', onClick: () => send() },
        duration: 10000,
      })
    } finally {
      setSending(false)
    }
  }

  const prePolishRef = useRef<null | string>(null)

  const [polishTone, setPolishTone] = useState('professional')

  const polish = async (tone?: string) => {
    const handle = composeRef.current
    if (!handle) return
    const text = handle.getMarkdown()
    if (!text.trim()) return
    prePolishRef.current = text
    setPolishing(true)
    try {
      const result = await postJson<PolishResult>('/mail/ai/polish', {
        text,
        tone: tone ?? polishTone,
      })
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
      const priorMessages = store.get(threadMessagesAtom).slice(0, -1).slice(-3)
      const threadContext =
        priorMessages.length > 0
          ? priorMessages
              .map((m) => `From: ${m.sender}\n${m.clean_text || m.text_body || ''}`)
              .join('\n---\n')
          : undefined
      const result = await postJson<ReplySuggestResult>('/mail/ai/reply-suggest', {
        original_body: originalBody,
        original_sender: originalFrom,
        original_subject: subject,
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
      {/* mode toggle + recipients */}
      <div className="border-border flex shrink-0 items-center gap-1 border-b px-4 py-2 select-none">
        {(Object.keys(MODE_LABELS) as ReplyMode[]).map((m) => (
          <button
            aria-pressed={mode === m}
            className={`focus-visible:ring-accent cursor-pointer rounded px-2.5 py-2 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none md:py-1 ${
              mode === m
                ? 'bg-accent/10 text-accent'
                : 'text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary'
            }`}
            key={m}
            onClick={() => handleModeChange(m)}
          >
            {MODE_LABELS[m]}
          </button>
        ))}
        {mode !== 'forward' && (
          <span
            className="text-fg-muted ml-auto min-w-0 truncate text-xs"
            title={mode === 'reply' ? replyRecipients : replyAllRecipients}
          >
            to {mode === 'reply' ? replyRecipients : replyAllRecipients}
          </span>
        )}
      </div>

      {/* forward: to field */}
      {mode === 'forward' && (
        <div className="border-border shrink-0 border-b px-4 py-2">
          <div className="flex items-center gap-2">
            <label className="text-fg-muted w-16 shrink-0 text-xs">To</label>
            <ContactAutocomplete
              className="border-border text-fg placeholder-fg-muted focus:border-accent w-full rounded-md border bg-transparent px-2 py-1 text-sm outline-none"
              onChange={setForwardTo}
              placeholder="recipient@example.com"
              value={forwardTo}
            />
          </div>
          {error && <p className="text-danger mt-1 text-xs">{error}</p>}
        </div>
      )}

      {/* AI suggestions */}
      {suggestions.length > 0 && (
        <div className="border-border flex shrink-0 flex-wrap gap-1.5 border-b px-4 py-2">
          {suggestions.map((s, i) => (
            <button
              className="border-border bg-accent/10 text-accent hover:bg-bg-secondary max-w-xs truncate rounded-full border px-2.5 py-0.5 text-xs transition-colors"
              key={i}
              onClick={() => applySuggestion(s)}
              title={s}
            >
              {s}
            </button>
          ))}
          <button
            className="text-fg-muted hover:text-fg-secondary rounded-full px-2 py-0.5 text-xs transition-colors"
            onClick={() => setSuggestions([])}
          >
            Dismiss
          </button>
        </div>
      )}

      {/* block-based composer */}
      <div className="min-h-0 flex-1 overflow-hidden">
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
      </div>

      {/* action bar */}
      <div className="border-border flex shrink-0 flex-wrap items-center gap-1 border-t px-4 py-2 select-none">
        {mode !== 'forward' && (
          <button
            className="text-accent hover:bg-accent/10 disabled:text-fg-muted flex h-8 shrink-0 items-center rounded-md px-2 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
            disabled={suggesting || sending}
            onClick={suggest}
            title="AI reply suggestions"
          >
            {suggesting ? <Loader2 className="h-3 w-3 animate-spin" /> : 'Suggest'}
          </button>
        )}
        <div className="relative flex shrink-0">
          <button
            className="text-accent hover:bg-accent/10 disabled:text-fg-muted flex h-8 items-center rounded-l-md px-2 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
            disabled={polishing || sending}
            onClick={() => polish()}
            title={`Polish (${polishTone})`}
          >
            {polishing ? <Loader2 className="h-3 w-3 animate-spin" /> : 'Polish'}
          </button>
          <select
            className="border-border text-accent hover:bg-accent/10 h-8 appearance-none rounded-r-md border-l bg-transparent px-1 text-[10px] outline-none disabled:cursor-not-allowed disabled:opacity-50"
            disabled={polishing || sending}
            onChange={(e) => setPolishTone(e.target.value)}
            value={polishTone}
          >
            <option value="professional">Pro</option>
            <option value="casual">Casual</option>
            <option value="formal">Formal</option>
            <option value="friendly">Friendly</option>
            <option value="concise">Concise</option>
          </select>
        </div>
        <div className="flex-1" />

        <button
          className="border-border text-fg hover:bg-bg-tertiary flex h-8 shrink-0 items-center gap-1.5 rounded-md border px-3 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          disabled={sending}
          onClick={() => {
            const content = composeRef.current?.getContent()
            const html = content?.fullHtml ?? ''
            setPreviewHtml(html)
          }}
          title="Preview as the recipient will see it"
        >
          <Eye className="h-3.5 w-3.5" />
          Preview
        </button>

        <button
          className="bg-accent hover:bg-accent-hover flex h-8 shrink-0 items-center gap-1.5 rounded-md px-3 text-xs font-medium text-white transition-all hover:shadow-md active:scale-95 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={sending}
          onClick={send}
        >
          <Send className="h-3.5 w-3.5" />
          {sending ? 'Sending…' : 'Send'}
        </button>
      </div>

      {previewHtml !== null && (
        <div
          className="bg-fg/40 fixed inset-0 z-50 flex items-center justify-center p-4 backdrop-blur-sm"
          onClick={() => setPreviewHtml(null)}
        >
          <div
            className="border-border bg-bg flex max-h-[85vh] w-full max-w-3xl flex-col rounded-lg border shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="border-border flex items-center justify-between border-b px-4 py-2">
              <div className="text-fg text-sm font-medium">Preview — receiver's view</div>
              <button
                className="text-fg-muted hover:text-fg text-xs"
                onClick={() => setPreviewHtml(null)}
              >
                Close
              </button>
            </div>
            <iframe
              className="bg-bg-secondary min-h-[60vh] flex-1 rounded-b-lg"
              sandbox=""
              srcDoc={`<!doctype html><html><head><meta charset="utf-8"><style>body{font-family:system-ui,-apple-system,sans-serif;font-size:14px;line-height:1.5;color:#222;background:#fff;padding:16px;margin:0}*{max-width:100%}</style></head><body>${previewHtml}</body></html>`}
              title="message preview"
            />
            <div className="border-border flex items-center justify-end gap-2 border-t px-4 py-2">
              <button
                className="border-border text-fg hover:bg-bg-tertiary rounded-md border px-3 py-1.5 text-xs"
                onClick={() => setPreviewHtml(null)}
              >
                Back to edit
              </button>
              <button
                className="bg-accent hover:bg-accent-hover rounded-md px-3 py-1.5 text-xs font-medium text-white"
                disabled={sending}
                onClick={() => {
                  setPreviewHtml(null)
                  void send()
                }}
              >
                Send
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
