import { useAtomValue, useSetAtom } from 'jotai'
import { File as FileIcon, Paperclip, Send, X } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { RichEditor, getEditorContent } from '@/components/rich-editor'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { ConversationSummary } from '@/lib/types'
import { authAtom } from '@/store/auth'
import { composingNewAtom, conversationsAtom, selectedThreadIdAtom } from '@/store/chat'
import type { Editor } from '@tiptap/react'

type SendResult = { success: boolean; message?: string; message_id?: string }
type TemplateInfo = {
  id: number
  name: string
  subject: string
  html_body: string
  text_body: string
  category: string
}
type PolishResult = { success: boolean; polished?: string; message?: string }

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)}KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
}

export function NewConversation() {
  const auth = useAtomValue(authAtom)
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
  const [error, setError] = useState('')
  const [templates, setTemplates] = useState<TemplateInfo[]>([])
  const [files, setFiles] = useState<File[]>([])
  const [scheduledAt, setScheduledAt] = useState('')
  const [showSchedulePicker, setShowSchedulePicker] = useState(false)
  const [requestReadReceipt, setRequestReadReceipt] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const editorRef = useRef<Editor | null>(null)

  useEffect(() => {
    fetchJson<TemplateInfo[]>('/mail/templates')
      .then(setTemplates)
      .catch(() => {})
  }, [])

  const applyTemplate = (t: TemplateInfo) => {
    setSubject(t.subject)
    if (editorRef.current && t.html_body) {
      editorRef.current.commands.setContent(t.html_body)
    }
  }

  const polish = async () => {
    const { text } = getEditorContent(editorRef.current)
    if (!text.trim()) return
    setPolishing(true)
    try {
      const result = await postJson<PolishResult>('/mail/ai/polish', { text })
      if (result.success && result.polished && editorRef.current) {
        editorRef.current.commands.setContent(`<p>${result.polished.replace(/\n/g, '</p><p>')}</p>`)
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

  const setEditorRef = useCallback((editor: Editor | null) => {
    editorRef.current = editor
  }, [])

  const send = async () => {
    const recipients = to
      .split(/[,;]/)
      .map((s) => s.trim())
      .filter(Boolean)
    if (recipients.length === 0) {
      setError('Recipient is required')
      return
    }

    const { text, html } = getEditorContent(editorRef.current)
    if (!text.trim() && files.length === 0) {
      setError('Message body is required')
      return
    }

    setError('')
    setSending(true)

    try {
      const ccList = cc.split(/[,;]/).map((s) => s.trim()).filter(Boolean)
      const bccList = bcc.split(/[,;]/).map((s) => s.trim()).filter(Boolean)

      let result: SendResult

      if (files.length > 0) {
        const formData = new FormData()
        formData.append('from', auth?.address ?? '')
        formData.append('subject', subject)
        formData.append('body', text)
        formData.append('html_body', html)
        for (const r of recipients) formData.append('to', r)
        for (const c of ccList) formData.append('cc', c)
        for (const b of bccList) formData.append('bcc', b)
        for (const f of files) formData.append('attachments', f)
        if (scheduledAt) formData.append('scheduled_at', new Date(scheduledAt).toISOString())
        if (requestReadReceipt) formData.append('request_read_receipt', 'true')

        const res = await fetch('/api/mail/send-multipart', {
          method: 'POST',
          headers: { Authorization: `Bearer ${auth?.token ?? ''}` },
          body: formData,
        })
        result = await res.json()
      } else {
        result = await postJson<SendResult>('/mail/send', {
          from: auth?.address ?? '',
          to: recipients,
          cc: ccList,
          bcc: bccList,
          subject,
          body: text,
          html_body: html,
          in_reply_to: null,
          ...(scheduledAt ? { scheduled_at: new Date(scheduledAt).toISOString() } : {}),
          ...(requestReadReceipt ? { request_read_receipt: true } : {}),
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
        const convos = await fetchJson<ConversationSummary[]>(
          '/conversations?limit=50',
        )
        setConversations(convos)
        if (convos.length > 0) {
          setSelectedThread(convos[0].thread_id)
        }
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
      {/* header */}
      <div className="flex shrink-0 items-center justify-between border-b border-[var(--color-border-default)] px-6 py-3">
        <h2 className="text-sm font-semibold text-[var(--color-text-primary)]">
          New Conversation
        </h2>
        <button
          onClick={() => setComposingNew(false)}
          className="rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
          aria-label="Close"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {/* error */}
      {error && (
        <div className="mx-6 mt-3 rounded-md bg-[var(--color-status-danger-subtle)] px-3 py-2 text-sm text-[var(--color-status-danger)]">
          {error}
        </div>
      )}

      {/* address fields */}
      <div className="flex shrink-0 flex-col border-b border-[var(--color-border-default)]">
        <div className="flex items-center border-b border-[var(--color-border-default)] px-6">
          <label className="w-12 shrink-0 text-xs text-[var(--color-text-tertiary)]">
            To
          </label>
          <ContactAutocomplete
            value={to}
            onChange={setTo}
            placeholder="recipient@example.com"
            autoFocus
          />
          {!showCcBcc && (
            <button
              onClick={() => setShowCcBcc(true)}
              className="shrink-0 text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
            >
              Cc/Bcc
            </button>
          )}
        </div>
        {showCcBcc && (
          <>
            <div className="flex items-center border-b border-[var(--color-border-default)] px-6">
              <label className="w-12 shrink-0 text-xs text-[var(--color-text-tertiary)]">
                Cc
              </label>
              <ContactAutocomplete
                value={cc}
                onChange={setCc}
                placeholder="cc@example.com"
              />
            </div>
            <div className="flex items-center border-b border-[var(--color-border-default)] px-6">
              <label className="w-12 shrink-0 text-xs text-[var(--color-text-tertiary)]">
                Bcc
              </label>
              <ContactAutocomplete
                value={bcc}
                onChange={setBcc}
                placeholder="bcc@example.com"
              />
            </div>
          </>
        )}
        <div className="flex items-center px-6">
          <label htmlFor="new-conv-subject" className="w-12 shrink-0 text-xs text-[var(--color-text-tertiary)]">
            Subject
          </label>
          <input
            id="new-conv-subject"
            type="text"
            value={subject}
            onChange={(e) => setSubject(e.target.value)}
            className="flex-1 bg-transparent py-2 text-sm text-[var(--color-text-primary)] outline-none focus-visible:underline focus-visible:decoration-[var(--color-brand-primary)]"
          />
        </div>
      </div>

      {/* editor */}
      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        <RichEditor
          onSubmit={send}
          placeholder="Write your message..."
          disabled={sending}
          minHeight="12rem"
          getEditorRef={setEditorRef}
        />
      </div>

      {/* attachments — below editor, near action bar */}
      {files.length > 0 && (
        <div className="flex max-h-24 shrink-0 flex-wrap gap-1.5 overflow-y-auto px-4 pb-2">
          {files.map((f, i) => (
            <div
              key={i}
              className="flex items-center gap-1 rounded-full border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] px-2 py-0.5 text-xs"
            >
              <FileIcon className="h-3 w-3 shrink-0 text-[var(--color-text-tertiary)]" />
              <span className="max-w-32 truncate text-[var(--color-text-secondary)]">{f.name}</span>
              <span className="text-[var(--color-text-tertiary)]">{formatFileSize(f.size)}</span>
              <button
                onClick={() => setFiles((prev) => prev.filter((_, j) => j !== i))}
                className="ml-0.5 rounded-full p-0.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
                aria-label={`Remove ${f.name}`}
              >
                <X className="h-3 w-3" />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* schedule badge */}
      {scheduledAt && (
        <div className="shrink-0 px-4 pb-2">
          <span className="inline-flex items-center gap-1 rounded-full bg-[var(--color-brand-subtle)] px-2.5 py-0.5 text-xs text-[var(--color-brand-primary)]">
            Scheduled: {new Date(scheduledAt).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })}
            <button
              onClick={() => { setScheduledAt(''); setShowSchedulePicker(false) }}
              className="ml-0.5 rounded-full p-0.5 hover:opacity-70"
              aria-label="Clear schedule"
            >
              <X className="h-3 w-3" />
            </button>
          </span>
        </div>
      )}

      {/* action bar */}
      <div className="flex shrink-0 select-none items-center gap-1 border-t border-[var(--color-border-default)] px-4 py-2">
        <button
          onClick={send}
          disabled={sending}
          className="flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-[var(--color-brand-primary)] px-4 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Send className="h-3.5 w-3.5" />
          {sending ? 'Sending…' : 'Send'}
        </button>

        <button
          onClick={() => setShowSchedulePicker((v) => !v)}
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors ${
            showSchedulePicker
              ? 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'
              : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)]'
          }`}
          title="Schedule send"
        >
          <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="12" cy="12" r="10" />
            <path strokeLinecap="round" d="M12 6v6l4 2" />
          </svg>
        </button>
        {showSchedulePicker && (
          <input
            type="datetime-local"
            value={scheduledAt}
            onChange={(e) => setScheduledAt(e.target.value)}
            min={new Date().toISOString().slice(0, 16)}
            aria-label="Schedule send time"
            className="h-8 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand-primary)]"
          />
        )}

        <div className="mx-0.5 h-4 w-px bg-[var(--color-border-default)]" />

        <button
          onClick={() => fileInputRef.current?.click()}
          className="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] focus-visible:ring-2 focus-visible:ring-[var(--color-brand-primary)] focus-visible:outline-none"
          title="Attach files"
          aria-label="Attach files"
        >
          <Paperclip className="h-4 w-4" />
        </button>
        <input
          ref={fileInputRef}
          type="file"
          multiple
          aria-label="Attach files"
          className="hidden"
          onChange={(e) => {
            const selected = Array.from(e.target.files ?? [])
            setFiles((prev) => [...prev, ...selected])
            e.target.value = ''
          }}
        />

        <button
          onClick={polish}
          disabled={polishing || sending}
          className="flex h-8 shrink-0 items-center rounded-md px-2 text-xs text-[var(--color-brand-primary)] transition-colors hover:bg-[var(--color-brand-subtle)] disabled:cursor-not-allowed disabled:opacity-50"
          title="AI polish your text"
        >
          {polishing ? 'Polishing…' : 'Polish'}
        </button>

        {templates.length > 0 && (
          <select
            onChange={(e) => {
              const t = templates.find((t) => t.id === Number(e.target.value))
              if (t) applyTemplate(t)
              e.target.value = ''
            }}
            defaultValue=""
            className="h-8 rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 text-xs text-[var(--color-text-secondary)]"
          >
            <option value="" disabled>
              Templates
            </option>
            {templates.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        )}

        <label className="ml-1 flex shrink-0 cursor-pointer items-center gap-1 text-[10px] text-[var(--color-text-tertiary)]">
          <input
            type="checkbox"
            checked={requestReadReceipt}
            onChange={(e) => setRequestReadReceipt(e.target.checked)}
            className="h-3 w-3 rounded border-[var(--color-border-default)]"
          />
          Receipt
        </label>

        <div className="flex-1" />

        <kbd className="mr-1 hidden select-none text-[10px] text-[var(--color-text-tertiary)] sm:inline">⌘↵</kbd>
        <button
          onClick={() => setComposingNew(false)}
          disabled={sending}
          className="flex h-8 shrink-0 items-center rounded-md px-3 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)] disabled:cursor-not-allowed disabled:opacity-50"
        >
          Cancel
        </button>
      </div>
    </div>
  )
}
