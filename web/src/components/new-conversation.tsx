import { useAtomValue, useSetAtom } from 'jotai'
import { useState } from 'react'
import { toast } from 'sonner'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { MarkdownEditor } from '@/components/markdown-editor'
import { fetchJson, postJson } from '@/lib/api'
import type { ConversationSummary } from '@/lib/types'
import { authAtom } from '@/store/auth'
import { composingNewAtom, conversationsAtom, selectedThreadIdAtom } from '@/store/chat'

type SendResult = { success: boolean; message?: string }

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
  const [body, setBody] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState('')

  const send = async () => {
    const recipients = to
      .split(/[,;]/)
      .map((s) => s.trim())
      .filter(Boolean)
    if (recipients.length === 0) {
      setError('Recipient is required')
      return
    }

    setError('')
    setSending(true)

    try {
      const ccList = cc.split(/[,;]/).map((s) => s.trim()).filter(Boolean)
      const bccList = bcc.split(/[,;]/).map((s) => s.trim()).filter(Boolean)

      const result = await postJson<SendResult>('/mail/send', {
        from: auth?.address ?? '',
        to: recipients,
        cc: ccList,
        bcc: bccList,
        subject,
        body,
        in_reply_to: null,
      })

      if (result.success) {
        toast.success('Message sent')
        // refresh conversations and select the new one
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
      <div className="flex items-center justify-between border-b border-zinc-200 px-6 py-3 dark:border-zinc-800">
        <h2 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
          New Conversation
        </h2>
        <button
          onClick={() => setComposingNew(false)}
          className="text-xs text-zinc-400 transition-colors hover:text-zinc-600 dark:hover:text-zinc-300"
        >
          Cancel
        </button>
      </div>

      {error && (
        <div className="mx-6 mt-3 rounded-md bg-red-50 px-3 py-2 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
          {error}
        </div>
      )}

      <div className="flex flex-col border-b border-zinc-200 dark:border-zinc-800">
        <div className="flex items-center border-b border-zinc-100 px-6 dark:border-zinc-800/50">
          <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
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
              className="shrink-0 text-xs text-zinc-400 transition-colors hover:text-zinc-600 dark:hover:text-zinc-300"
            >
              Cc/Bcc
            </button>
          )}
        </div>
        {showCcBcc && (
          <>
            <div className="flex items-center border-b border-zinc-100 px-6 dark:border-zinc-800/50">
              <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
                Cc
              </label>
              <ContactAutocomplete
                value={cc}
                onChange={setCc}
                placeholder="cc@example.com"
              />
            </div>
            <div className="flex items-center border-b border-zinc-100 px-6 dark:border-zinc-800/50">
              <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
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
          <label htmlFor="new-conv-subject" className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
            Subject
          </label>
          <input
            id="new-conv-subject"
            type="text"
            value={subject}
            onChange={(e) => setSubject(e.target.value)}
            className="flex-1 bg-transparent py-2 text-sm text-zinc-900 outline-none dark:text-zinc-100"
          />
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        <MarkdownEditor
          value={body}
          onChange={setBody}
          onSubmit={send}
          placeholder="Write your message... (Markdown supported)"
          disabled={sending}
          minRows={6}
        />
      </div>

      <div className="flex gap-2 border-t border-zinc-200 p-4 dark:border-zinc-800">
        <button
          onClick={send}
          disabled={sending}
          className="rounded-md bg-blue-500 px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-blue-600 disabled:opacity-50"
        >
          {sending ? 'Sending...' : 'Send'}
        </button>
        <button
          onClick={() => setComposingNew(false)}
          disabled={sending}
          className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm transition-colors hover:bg-zinc-200 disabled:opacity-50 dark:bg-zinc-800 dark:hover:bg-zinc-700"
        >
          Cancel
        </button>
      </div>
    </div>
  )
}
