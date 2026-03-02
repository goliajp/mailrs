import { useAtomValue } from 'jotai'
import { useRef, useState } from 'react'

import { MarkdownEditor } from '@/components/markdown-editor'
import { postJson } from '@/lib/api'
import { authAtom } from '@/store/auth'

type SendResult = { success: boolean; message?: string }

export function ReplyBox({
  lastMessageId,
  recipients,
  subject,
  onSent,
}: {
  threadId: string
  lastMessageId: string
  recipients: string
  subject: string
  onSent: () => void
}) {
  const auth = useAtomValue(authAtom)
  const [body, setBody] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState('')
  const [files, setFiles] = useState<File[]>([])
  const fileInputRef = useRef<HTMLInputElement>(null)

  const removeFile = (index: number) => {
    setFiles((prev) => prev.filter((_, i) => i !== index))
  }

  const send = async () => {
    if (!body.trim() && files.length === 0) return
    setError('')
    setSending(true)

    try {
      const to = recipients
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)

      if (files.length > 0) {
        const formData = new FormData()
        formData.append('from', auth?.address ?? '')
        formData.append('subject', subject.startsWith('Re:') ? subject : `Re: ${subject}`)
        formData.append('body', body)
        formData.append('in_reply_to', lastMessageId)
        for (const r of to) formData.append('to', r)
        for (const f of files) formData.append('attachments', f)

        const res = await fetch('/api/mail/send-multipart', {
          method: 'POST',
          headers: { Authorization: `Bearer ${auth?.token ?? ''}` },
          body: formData,
        })
        const result: SendResult = await res.json()
        if (!result.success) {
          setError(result.message ?? 'Send failed')
          return
        }
      } else {
        const result = await postJson<SendResult>('/mail/send', {
          from: auth?.address ?? '',
          to,
          cc: [],
          bcc: [],
          subject: subject.startsWith('Re:') ? subject : `Re: ${subject}`,
          body,
          in_reply_to: lastMessageId,
        })
        if (!result.success) {
          setError(result.message ?? 'Send failed')
          return
        }
      }

      setBody('')
      setFiles([])
      onSent()
    } catch {
      setError('Network error')
    } finally {
      setSending(false)
    }
  }

  return (
    <div className="border-t border-zinc-200 dark:border-zinc-800">
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
            placeholder="Type a message... (Markdown supported)"
            disabled={sending}
          />
        </div>

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
