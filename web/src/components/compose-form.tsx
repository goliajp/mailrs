import { useAtom, useAtomValue } from 'jotai'
import { useState } from 'react'

import { postJson } from '@/lib/api'
import { authAtom } from '@/store/auth'
import type { ComposeState } from '@/store/mail'
import { composingAtom } from '@/store/mail'

type SendResult = { success: boolean; message?: string }

export function ComposeForm() {
  const [compose, setCompose] = useAtom(composingAtom)
  const auth = useAtomValue(authAtom)
  const [form, setForm] = useState<ComposeState>(
    compose ?? { to: '', cc: '', bcc: '', subject: '', body: '' }
  )
  const [showCc, setShowCc] = useState(!!form.cc || !!form.bcc)
  const [sending, setSending] = useState(false)
  const [error, setError] = useState('')

  const update = (field: keyof ComposeState, value: string) =>
    setForm((prev) => ({ ...prev, [field]: value }))

  const discard = () => setCompose(null)

  const parseRecipients = (s: string) =>
    s
      .split(/[,;]/)
      .map((r) => r.trim())
      .filter(Boolean)

  const send = async () => {
    if (!form.to.trim()) {
      setError('Recipient is required')
      return
    }

    setError('')
    setSending(true)

    try {
      const result = await postJson<SendResult>('/mail/send', {
        from: auth?.address ?? '',
        to: parseRecipients(form.to),
        cc: parseRecipients(form.cc),
        bcc: parseRecipients(form.bcc),
        subject: form.subject,
        body: form.body,
        in_reply_to: form.replyTo ?? null,
      })

      if (result.success) {
        setCompose(null)
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
      <div className="flex items-center justify-between border-b border-zinc-200 px-6 py-3 dark:border-zinc-800">
        <h2 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
          {form.replyTo ? 'Reply' : 'New Message'}
        </h2>
        <button
          onClick={discard}
          className="text-xs text-zinc-400 transition-colors hover:text-zinc-600 dark:hover:text-zinc-300"
        >
          Discard
        </button>
      </div>

      {error && (
        <div className="mx-6 mt-3 rounded-md bg-red-50 px-3 py-2 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
          {error}
        </div>
      )}

      <div className="flex flex-col gap-0 border-b border-zinc-200 dark:border-zinc-800">
        <div className="flex items-center border-b border-zinc-100 px-6 dark:border-zinc-800/50">
          <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
            To
          </label>
          <input
            type="text"
            value={form.to}
            onChange={(e) => update('to', e.target.value)}
            className="flex-1 bg-transparent py-2 text-sm text-zinc-900 outline-none dark:text-zinc-100"
            placeholder="recipient@example.com"
            autoFocus
          />
          {!showCc && (
            <button
              onClick={() => setShowCc(true)}
              className="text-xs text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300"
            >
              Cc/Bcc
            </button>
          )}
        </div>

        {showCc && (
          <>
            <div className="flex items-center border-b border-zinc-100 px-6 dark:border-zinc-800/50">
              <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
                Cc
              </label>
              <input
                type="text"
                value={form.cc}
                onChange={(e) => update('cc', e.target.value)}
                className="flex-1 bg-transparent py-2 text-sm text-zinc-900 outline-none dark:text-zinc-100"
              />
            </div>
            <div className="flex items-center border-b border-zinc-100 px-6 dark:border-zinc-800/50">
              <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
                Bcc
              </label>
              <input
                type="text"
                value={form.bcc}
                onChange={(e) => update('bcc', e.target.value)}
                className="flex-1 bg-transparent py-2 text-sm text-zinc-900 outline-none dark:text-zinc-100"
              />
            </div>
          </>
        )}

        <div className="flex items-center px-6">
          <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
            Subject
          </label>
          <input
            type="text"
            value={form.subject}
            onChange={(e) => update('subject', e.target.value)}
            className="flex-1 bg-transparent py-2 text-sm text-zinc-900 outline-none dark:text-zinc-100"
          />
        </div>
      </div>

      <textarea
        value={form.body}
        onChange={(e) => update('body', e.target.value)}
        className="flex-1 resize-none bg-transparent p-6 text-sm leading-relaxed text-zinc-900 outline-none dark:text-zinc-100"
        placeholder="Write your message..."
      />

      <div className="flex gap-2 border-t border-zinc-200 p-4 dark:border-zinc-800">
        <button
          onClick={send}
          disabled={sending}
          className="rounded-md bg-zinc-900 px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
        >
          {sending ? 'Sending...' : 'Send'}
        </button>
        <button
          onClick={discard}
          disabled={sending}
          className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm transition-colors hover:bg-zinc-200 disabled:opacity-50 dark:bg-zinc-800 dark:hover:bg-zinc-700"
        >
          Discard
        </button>
      </div>
    </div>
  )
}
