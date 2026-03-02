import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef, useState } from 'react'

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
  const [subject, setSubject] = useState('')
  const [body, setBody] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState('')
  const [suggestions, setSuggestions] = useState<string[]>([])
  const [showSuggestions, setShowSuggestions] = useState(false)
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null)

  // contact autocomplete
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    const query = to.split(/[,;]/).pop()?.trim() ?? ''
    if (query.length < 2) {
      setSuggestions([])
      return
    }
    debounceRef.current = setTimeout(async () => {
      try {
        const data = await fetchJson<string[]>(
          `/contacts?q=${encodeURIComponent(query)}&limit=5`
        )
        setSuggestions(data)
        setShowSuggestions(data.length > 0)
      } catch {
        setSuggestions([])
      }
    }, 300)
  }, [to])

  const selectSuggestion = useCallback(
    (s: string) => {
      const parts = to.split(/[,;]/)
      parts.pop()
      parts.push(s)
      setTo(parts.join(', ') + ', ')
      setShowSuggestions(false)
    },
    [to]
  )

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
      const result = await postJson<SendResult>('/mail/send', {
        from: auth?.address ?? '',
        to: recipients,
        cc: [],
        bcc: [],
        subject,
        body,
        in_reply_to: null,
      })

      if (result.success) {
        // refresh conversations and select the new one
        const convos = await fetchJson<ConversationSummary[]>(
          '/conversations?limit=50'
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
        <div className="relative flex items-center border-b border-zinc-100 px-6 dark:border-zinc-800/50">
          <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
            To
          </label>
          <input
            type="text"
            value={to}
            onChange={(e) => setTo(e.target.value)}
            onFocus={() => suggestions.length > 0 && setShowSuggestions(true)}
            onBlur={() => setTimeout(() => setShowSuggestions(false), 150)}
            className="flex-1 bg-transparent py-2 text-sm text-zinc-900 outline-none dark:text-zinc-100"
            placeholder="recipient@example.com"
            autoFocus
          />
          {showSuggestions && (
            <div className="absolute left-12 top-full z-10 mt-1 w-72 rounded-md border border-zinc-200 bg-white shadow-lg dark:border-zinc-700 dark:bg-zinc-900">
              {suggestions.map((s) => (
                <button
                  key={s}
                  onMouseDown={() => selectSuggestion(s)}
                  className="w-full px-3 py-2 text-left text-sm text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800"
                >
                  {s}
                </button>
              ))}
            </div>
          )}
        </div>
        <div className="flex items-center px-6">
          <label className="w-12 shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
            Subject
          </label>
          <input
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
