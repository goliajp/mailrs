import { useAtomValue } from 'jotai'
import { useCallback, useRef, useState } from 'react'
import { toast } from 'sonner'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { RichEditor, getEditorContent } from '@/components/rich-editor'
import { postJson, saveDraft } from '@/lib/api'
import { authAtom } from '@/store/auth'
import { appendSignature, signatureAtom, signatureEnabledAtom } from '@/store/settings'
import type { Editor } from '@tiptap/react'

export type ReplyMode = 'reply' | 'reply-all' | 'forward'

type SendResult = { success: boolean; message?: string }
type ReplySuggestResult = { success: boolean; suggestions: string[]; message?: string }
type PolishResult = { success: boolean; polished?: string; message?: string }

const MODE_LABELS: Record<ReplyMode, string> = {
  reply: 'Reply',
  'reply-all': 'Reply All',
  forward: 'Forward',
}

export function ReplyBox({
  threadId,
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
  const [forwardTo, setForwardTo] = useState('')
  const [sending, setSending] = useState(false)
  const [savingDraft, setSavingDraft] = useState(false)
  const [polishing, setPolishing] = useState(false)
  const [suggesting, setSuggesting] = useState(false)
  const [suggestions, setSuggestions] = useState<string[]>([])
  const [error, setError] = useState('')
  const [files, setFiles] = useState<File[]>([])
  const fileInputRef = useRef<HTMLInputElement>(null)
  const editorRef = useRef<Editor | null>(null)

  const setEditorRef = useCallback((editor: Editor | null) => {
    editorRef.current = editor
  }, [])

  const handleModeChange = (newMode: ReplyMode) => {
    onModeChange(newMode)
    if (newMode === 'forward' && editorRef.current) {
      const { text } = getEditorContent(editorRef.current)
      if (!text.trim()) {
        const fwdHtml = `<br><br><p>---------- Forwarded message ----------</p><p>From: ${originalFrom}</p><p>Date: ${originalDate}</p><p>Subject: ${subject}</p><br>${originalBody}`
        editorRef.current.commands.setContent(fwdHtml)
      }
    }
  }

  const removeFile = (index: number) => {
    setFiles((prev) => prev.filter((_, i) => i !== index))
  }

  const resolveRecipients = (): string[] => {
    if (mode === 'reply') {
      return replyRecipients.split(',').map((s) => s.trim()).filter(Boolean)
    }
    if (mode === 'reply-all') {
      return replyAllRecipients.split(',').map((s) => s.trim()).filter(Boolean)
    }
    return forwardTo.split(',').map((s) => s.trim()).filter(Boolean)
  }

  const resolveSubject = (): string => {
    if (mode === 'forward') {
      return subject.startsWith('Fwd:') ? subject : `Fwd: ${subject}`
    }
    return subject.startsWith('Re:') ? subject : `Re: ${subject}`
  }

  const send = async () => {
    const to = resolveRecipients()
    if (mode === 'forward' && to.length === 0) {
      setError('Please enter at least one recipient')
      return
    }

    const { text, html } = getEditorContent(editorRef.current)
    if (!text.trim() && files.length === 0) return

    setError('')
    setSending(true)

    const resolvedSubject = resolveSubject()
    const bodyWithSig = appendSignature(text, signature, signatureEnabled)
    const inReplyTo = mode === 'forward' ? undefined : lastMessageId

    try {
      if (files.length > 0) {
        const formData = new FormData()
        formData.append('from', auth?.address ?? '')
        formData.append('subject', resolvedSubject)
        formData.append('body', bodyWithSig)
        formData.append('html_body', html)
        if (inReplyTo) formData.append('in_reply_to', inReplyTo)
        for (const r of to) formData.append('to', r)
        for (const f of files) formData.append('attachments', f)

        const res = await fetch('/api/mail/send-multipart', {
          method: 'POST',
          headers: { Authorization: `Bearer ${auth?.token ?? ''}` },
          body: formData,
        })
        const result: SendResult = await res.json()
        if (!result.success) {
          const msg = result.message ?? 'Send failed'
          setError(msg)
          toast.error(msg)
          return
        }
      } else {
        const payload: Record<string, unknown> = {
          from: auth?.address ?? '',
          to,
          cc: [],
          bcc: [],
          subject: resolvedSubject,
          body: bodyWithSig,
          html_body: html,
        }
        if (inReplyTo) payload['in_reply_to'] = inReplyTo

        const result = await postJson<SendResult>('/mail/send', payload)
        if (!result.success) {
          const msg = result.message ?? 'Send failed'
          setError(msg)
          toast.error(msg)
          return
        }
      }

      // clear editor
      editorRef.current?.commands.clearContent()
      setForwardTo('')
      setFiles([])
      toast.success(mode === 'forward' ? 'Forwarded' : 'Reply sent')
      onSent()
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Network error'
      setError(msg)
      toast.error(msg)
    } finally {
      setSending(false)
    }
  }

  const handleSaveDraft = async () => {
    const { text } = getEditorContent(editorRef.current)
    if (!text.trim()) return
    setSavingDraft(true)
    try {
      const to = resolveRecipients().join(', ')
      const resolvedSubject = resolveSubject()
      const bodyWithSig = appendSignature(text, signature, signatureEnabled)
      const result = await saveDraft({
        to,
        subject: resolvedSubject,
        body: bodyWithSig,
        reply_to_thread_id: threadId,
      })
      if (result.success) {
        toast.success('Draft saved')
      } else {
        toast.error(result.message ?? 'Failed to save draft')
      }
    } catch {
      toast.error('Failed to save draft')
    } finally {
      setSavingDraft(false)
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
      }
    } catch {
      toast.error('AI unavailable')
    } finally {
      setPolishing(false)
    }
  }

  const suggest = async () => {
    setSuggesting(true)
    try {
      const result = await postJson<ReplySuggestResult>('/mail/ai/reply-suggest', {
        original_sender: originalFrom,
        original_subject: subject,
        original_body: originalBody,
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

  const applySuggestion = (text: string) => {
    if (editorRef.current) {
      editorRef.current.commands.setContent(`<p>${text.replace(/\n/g, '</p><p>')}</p>`)
    }
    setSuggestions([])
  }

  const placeholder =
    mode === 'forward' ? 'Add a message...' : 'Type a reply...'

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* mode toggle */}
      <div className="flex shrink-0 items-center gap-1 px-3 pt-2">
        {(Object.keys(MODE_LABELS) as ReplyMode[]).map((m) => (
          <button
            key={m}
            onClick={() => handleModeChange(m)}
            className={`rounded px-2 py-0.5 text-xs font-medium transition-colors ${
              mode === m
                ? 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300'
                : 'text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300'
            }`}
          >
            {MODE_LABELS[m]}
          </button>
        ))}
        {mode !== 'forward' && (
          <span className="ml-1 truncate text-xs text-zinc-400" title={mode === 'reply' ? replyRecipients : replyAllRecipients}>
            to {mode === 'reply' ? replyRecipients : replyAllRecipients}
          </span>
        )}
      </div>

      {/* forward: to field */}
      {mode === 'forward' && (
        <div className="px-3 pt-1.5">
          <ContactAutocomplete
            value={forwardTo}
            onChange={setForwardTo}
            placeholder="To: recipient@example.com, ..."
            className="w-full rounded-md border border-zinc-200 bg-transparent px-2 py-1 text-sm text-zinc-800 placeholder-zinc-400 outline-none focus:border-indigo-400 dark:border-zinc-700 dark:text-zinc-200"
          />
        </div>
      )}

      {error && (
        <div className="mx-4 mt-2 rounded-md bg-red-50 px-3 py-1.5 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
          {error}
        </div>
      )}

      {files.length > 0 && (
        <div className="flex flex-wrap gap-2 px-4 pt-2">
          {files.map((f, i) => (
            <div
              key={i}
              className="flex items-center gap-1.5 rounded-md bg-zinc-100 px-2.5 py-1 text-xs dark:bg-zinc-800"
            >
              <svg className="h-3.5 w-3.5 shrink-0 text-zinc-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
              </svg>
              <span className="max-w-32 truncate text-zinc-700 dark:text-zinc-300">{f.name}</span>
              <span className="text-zinc-400">({(f.size / 1024).toFixed(0)}KB)</span>
              <button
                onClick={() => removeFile(i)}
                className="ml-0.5 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200"
              >
                &times;
              </button>
            </div>
          ))}
        </div>
      )}

      {/* AI reply suggestions */}
      {suggestions.length > 0 && (
        <div className="flex flex-wrap gap-1.5 px-4 pb-1 pt-2">
          {suggestions.map((s, i) => (
            <button
              key={i}
              onClick={() => applySuggestion(s)}
              className="max-w-xs truncate rounded-md border border-purple-200 bg-purple-50 px-2 py-1 text-xs text-purple-700 transition-colors hover:bg-purple-100 dark:border-purple-800 dark:bg-purple-900/20 dark:text-purple-300 dark:hover:bg-purple-900/40"
              title={s}
            >
              {s.slice(0, 80)}{s.length > 80 ? '...' : ''}
            </button>
          ))}
          <button
            onClick={() => setSuggestions([])}
            className="rounded-md px-1.5 py-1 text-xs text-zinc-400 hover:text-zinc-600"
          >
            Dismiss
          </button>
        </div>
      )}

      {/* editor — fills remaining space */}
      <div className="min-h-0 flex-1 overflow-y-auto px-3 pt-2">
        <RichEditor
          onSubmit={send}
          placeholder={placeholder}
          disabled={sending}
          minHeight="100%"
          getEditorRef={setEditorRef}
        />
      </div>

      {/* action bar */}
      <div className="flex shrink-0 items-center gap-1 px-3 pb-2 pt-1.5">
        <button
          onClick={() => fileInputRef.current?.click()}
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-zinc-400 transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800"
          title="Attach file"
        >
          <svg
            className="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M18.375 12.739l-7.693 7.693a4.5 4.5 0 01-6.364-6.364l10.94-10.94A3 3 0 1119.5 7.372L8.552 18.32m.009-.01l-.01.01m5.699-9.941l-7.81 7.81a1.5 1.5 0 002.112 2.13"
            />
          </svg>
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

        {mode !== 'forward' && (
          <button
            onClick={suggest}
            disabled={suggesting}
            className="flex h-7 shrink-0 items-center justify-center rounded-md px-2 text-xs text-purple-500 transition-colors hover:bg-purple-50 hover:text-purple-700 disabled:opacity-40 dark:hover:bg-purple-900/30 dark:hover:text-purple-300"
            title="AI reply suggestions"
          >
            {suggesting ? '...' : 'Suggest'}
          </button>
        )}
        <button
          onClick={polish}
          disabled={polishing}
          className="flex h-7 shrink-0 items-center justify-center rounded-md px-2 text-xs text-purple-500 transition-colors hover:bg-purple-50 hover:text-purple-700 disabled:opacity-40 dark:hover:bg-purple-900/30 dark:hover:text-purple-300"
          title="AI polish text"
        >
          {polishing ? '...' : 'Polish'}
        </button>
        <button
          onClick={handleSaveDraft}
          disabled={savingDraft}
          className="flex h-7 shrink-0 items-center justify-center rounded-md px-2 text-xs text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 disabled:opacity-40 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title="Save draft"
        >
          {savingDraft ? 'Saving...' : 'Draft'}
        </button>

        <div className="flex-1" />

        <button
          onClick={send}
          disabled={sending}
          className="flex h-7 shrink-0 items-center gap-1.5 rounded-xl bg-indigo-500 px-3 text-xs font-medium text-white transition-colors hover:bg-indigo-600 disabled:opacity-40"
          title="Send (Ctrl+Enter)"
        >
          <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor">
            <path d="M3.478 2.405a.75.75 0 00-.926.94l2.432 7.905H13.5a.75.75 0 010 1.5H4.984l-2.432 7.905a.75.75 0 00.926.94 60.519 60.519 0 0018.445-8.986.75.75 0 000-1.218A60.517 60.517 0 003.478 2.405z" />
          </svg>
          {sending ? 'Sending...' : 'Send'}
        </button>
      </div>
    </div>
  )
}
