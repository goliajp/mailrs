import { useAtomValue } from 'jotai'
import { useRef, useState } from 'react'
import { toast } from 'sonner'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { MarkdownEditor } from '@/components/markdown-editor'
import { postJson, saveDraft } from '@/lib/api'
import { authAtom } from '@/store/auth'
import { appendSignature, signatureAtom, signatureEnabledAtom } from '@/store/settings'

export type ReplyMode = 'reply' | 'reply-all' | 'forward'

type SendResult = { success: boolean; message?: string }

const MODE_LABELS: Record<ReplyMode, string> = {
  reply: 'Reply',
  'reply-all': 'Reply All',
  forward: 'Forward',
}

function buildForwardBody(
  originalFrom: string,
  originalDate: string,
  originalSubject: string,
  originalBody: string,
): string {
  return `\n\n---------- Forwarded message ----------\nFrom: ${originalFrom}\nDate: ${originalDate}\nSubject: ${originalSubject}\n\n${originalBody}`
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
  const [body, setBody] = useState('')
  const [forwardTo, setForwardTo] = useState('')
  const [sending, setSending] = useState(false)
  const [savingDraft, setSavingDraft] = useState(false)
  const [error, setError] = useState('')
  const [files, setFiles] = useState<File[]>([])
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleModeChange = (newMode: ReplyMode) => {
    onModeChange(newMode)
    // pre-fill body for forward mode
    if (newMode === 'forward' && body === '') {
      setBody(buildForwardBody(originalFrom, originalDate, subject, originalBody))
    } else if (newMode !== 'forward') {
      // clear forward pre-fill if user switches away and body is only the template
      const forwardTemplate = buildForwardBody(originalFrom, originalDate, subject, originalBody)
      if (body === forwardTemplate) {
        setBody('')
      }
    }
  }

  const removeFile = (index: number) => {
    setFiles((prev) => prev.filter((_, i) => i !== index))
  }

  const resolveRecipients = (): string[] => {
    if (mode === 'reply') {
      return replyRecipients
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
    }
    if (mode === 'reply-all') {
      return replyAllRecipients
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
    }
    // forward
    return forwardTo
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
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
    if (!body.trim() && files.length === 0) return
    setError('')
    setSending(true)

    const resolvedSubject = resolveSubject()
    const bodyWithSig = appendSignature(body, signature, signatureEnabled)
    // forward is not a reply — omit in_reply_to
    const inReplyTo = mode === 'forward' ? undefined : lastMessageId

    try {
      if (files.length > 0) {
        const formData = new FormData()
        formData.append('from', auth?.address ?? '')
        formData.append('subject', resolvedSubject)
        formData.append('body', bodyWithSig)
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

      setBody('')
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
    if (!body.trim()) return
    setSavingDraft(true)
    try {
      const to = resolveRecipients().join(', ')
      const resolvedSubject = resolveSubject()
      const bodyWithSig = appendSignature(body, signature, signatureEnabled)
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

  const placeholder =
    mode === 'forward'
      ? 'Add a message... (Markdown supported)'
      : 'Type a reply... (Markdown supported)'

  return (
    <div className="border-t border-zinc-200 dark:border-zinc-800">
      {/* mode toggle */}
      <div className="flex items-center gap-1 px-3 pt-2">
        {(Object.keys(MODE_LABELS) as ReplyMode[]).map((m) => (
          <button
            key={m}
            onClick={() => handleModeChange(m)}
            className={`rounded px-2 py-0.5 text-xs font-medium transition-colors ${
              mode === m
                ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300'
                : 'text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300'
            }`}
          >
            {MODE_LABELS[m]}
          </button>
        ))}
        {/* recipient preview for reply/reply-all */}
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
            className="w-full rounded-md border border-zinc-200 bg-transparent px-2 py-1 text-sm text-zinc-800 placeholder-zinc-400 outline-none focus:border-blue-400 dark:border-zinc-700 dark:text-zinc-200"
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
              className="flex items-center gap-1 rounded-md bg-zinc-100 px-2 py-1 text-xs dark:bg-zinc-800"
            >
              <span className="max-w-32 truncate">{f.name}</span>
              <button
                onClick={() => removeFile(i)}
                className="text-zinc-400 hover:text-zinc-600"
              >
                x
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="flex items-end gap-2 p-3">
        <button
          onClick={() => fileInputRef.current?.click()}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-zinc-400 transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800"
          title="Attach file"
        >
          <svg
            className="h-5 w-5"
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
          onDrop={(e) => {
            e.preventDefault()
            const dropped = Array.from(e.dataTransfer.files)
            setFiles((prev) => [...prev, ...dropped])
          }}
        />

        <div className="flex-1">
          <MarkdownEditor
            value={body}
            onChange={setBody}
            onSubmit={send}
            placeholder={placeholder}
            disabled={sending}
          />
        </div>

        <button
          onClick={handleSaveDraft}
          disabled={savingDraft || !body.trim()}
          className="flex h-8 shrink-0 items-center justify-center rounded-md px-2 text-xs text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 disabled:opacity-40 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title="Save draft"
        >
          {savingDraft ? 'Saving...' : 'Draft'}
        </button>

        <button
          onClick={send}
          disabled={sending || (!body.trim() && files.length === 0)}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-blue-500 text-white transition-colors hover:bg-blue-600 disabled:opacity-40"
          title="Send (Ctrl+Enter)"
        >
          <svg className="h-4 w-4" viewBox="0 0 24 24" fill="currentColor">
            <path d="M3.478 2.405a.75.75 0 00-.926.94l2.432 7.905H13.5a.75.75 0 010 1.5H4.984l-2.432 7.905a.75.75 0 00.926.94 60.519 60.519 0 0018.445-8.986.75.75 0 000-1.218A60.517 60.517 0 003.478 2.405z" />
          </svg>
        </button>
      </div>
    </div>
  )
}
