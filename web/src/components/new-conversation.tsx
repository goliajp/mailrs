import { useAtomValue, useSetAtom } from 'jotai'
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
      <div className="flex items-center justify-between border-b border-[var(--color-border-default)] px-6 py-3">
        <h2 className="text-sm font-semibold text-[var(--color-text-primary)]">
          New Conversation
        </h2>
        <button
          onClick={() => setComposingNew(false)}
          className="text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
        >
          Cancel
        </button>
      </div>

      {error && (
        <div className="mx-6 mt-3 rounded-md bg-[var(--color-status-danger-subtle)] px-3 py-2 text-sm text-[var(--color-status-danger)]">
          {error}
        </div>
      )}

      <div className="flex flex-col border-b border-[var(--color-border-default)]">
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
            className="flex-1 bg-transparent py-2 text-sm text-[var(--color-text-primary)] outline-none"
          />
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        <RichEditor
          onSubmit={send}
          placeholder="Write your message..."
          disabled={sending}
          minHeight="12rem"
          getEditorRef={setEditorRef}
        />

        {files.length > 0 && (
          <div className="mt-3 flex flex-wrap gap-2">
            {files.map((f, i) => (
              <div
                key={i}
                className="flex items-center gap-1.5 rounded-md bg-[var(--color-bg-raised)] px-2.5 py-1 text-xs"
              >
                <svg className="h-3.5 w-3.5 shrink-0 text-[var(--color-text-tertiary)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                </svg>
                <span className="max-w-40 truncate text-[var(--color-text-secondary)]">{f.name}</span>
                <span className="text-[var(--color-text-tertiary)]">({(f.size / 1024).toFixed(0)}KB)</span>
                <button
                  onClick={() => setFiles((prev) => prev.filter((_, j) => j !== i))}
                  className="ml-0.5 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
                >
                  &times;
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="flex items-center gap-2 px-6 pb-1">
        <label className="flex items-center gap-1.5 text-xs text-[var(--color-text-secondary)]">
          <input
            type="checkbox"
            checked={requestReadReceipt}
            onChange={(e) => setRequestReadReceipt(e.target.checked)}
            className="h-3.5 w-3.5 rounded border-[var(--color-border-default)]"
          />
          Request read receipt
        </label>
      </div>

      <div className="flex items-center gap-2 border-t border-[var(--color-border-default)] p-4">
        <button
          onClick={send}
          disabled={sending}
          className="rounded-md bg-[var(--color-brand-primary)] px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-50"
        >
          {sending ? 'Sending...' : 'Send'}
        </button>
        <button
          onClick={() => setShowSchedulePicker((v) => !v)}
          className="rounded-md bg-[var(--color-bg-raised)] px-2 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
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
            className="rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 py-1 text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand-primary)]"
          />
        )}
        {scheduledAt && (
          <span className="flex items-center gap-1 rounded-full bg-[var(--color-brand-subtle)] px-2 py-0.5 text-xs text-[var(--color-brand-primary)]">
            Scheduled: {new Date(scheduledAt).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })}
            <button
              onClick={() => { setScheduledAt(''); setShowSchedulePicker(false) }}
              className="ml-0.5 hover:opacity-70"
            >
              &times;
            </button>
          </span>
        )}
        <button
          onClick={() => fileInputRef.current?.click()}
          className="rounded-md bg-[var(--color-bg-raised)] px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
          title="Attach files"
        >
          Attach
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
          className="rounded-md bg-[var(--color-brand-subtle)] px-3 py-1.5 text-sm text-[var(--color-brand-primary)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
          title="AI polish your text"
        >
          {polishing ? 'Polishing...' : 'AI Polish'}
        </button>
        {templates.length > 0 && (
          <select
            onChange={(e) => {
              const t = templates.find((t) => t.id === Number(e.target.value))
              if (t) applyTemplate(t)
              e.target.value = ''
            }}
            defaultValue=""
            className="rounded-md bg-[var(--color-bg-raised)] px-2 py-1.5 text-sm text-[var(--color-text-secondary)]"
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
        <div className="flex-1" />
        <button
          onClick={() => setComposingNew(false)}
          disabled={sending}
          className="rounded-md bg-[var(--color-bg-raised)] px-3 py-1.5 text-sm transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
        >
          Cancel
        </button>
      </div>
    </div>
  )
}
