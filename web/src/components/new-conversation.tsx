import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { ContactAutocomplete } from '@/components/contact-autocomplete'
import { RichEditor, getEditorContent } from '@/components/rich-editor'
import { fetchJson, postJson } from '@/lib/api'
import type { ConversationSummary } from '@/lib/types'
import { authAtom } from '@/store/auth'
import { composingNewAtom, conversationsAtom, selectedThreadIdAtom } from '@/store/chat'
import type { Editor } from '@tiptap/react'

type SendResult = { success: boolean; message?: string }
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
    if (!text.trim()) {
      setError('Message body is required')
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
        body: text,
        html_body: html,
        in_reply_to: null,
      })

      if (result.success) {
        toast.success('Message sent')
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
        <RichEditor
          onSubmit={send}
          placeholder="Write your message..."
          disabled={sending}
          minHeight="12rem"
          getEditorRef={setEditorRef}
        />
      </div>

      <div className="flex items-center gap-2 border-t border-zinc-200 p-4 dark:border-zinc-800">
        <button
          onClick={send}
          disabled={sending}
          className="rounded-md bg-blue-500 px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-blue-600 disabled:opacity-50"
        >
          {sending ? 'Sending...' : 'Send'}
        </button>
        <button
          onClick={polish}
          disabled={polishing || sending}
          className="rounded-md bg-purple-100 px-3 py-1.5 text-sm text-purple-700 transition-colors hover:bg-purple-200 disabled:opacity-50 dark:bg-purple-900/30 dark:text-purple-300 dark:hover:bg-purple-900/50"
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
            className="rounded-md bg-zinc-100 px-2 py-1.5 text-sm text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400"
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
          className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm transition-colors hover:bg-zinc-200 disabled:opacity-50 dark:bg-zinc-800 dark:hover:bg-zinc-700"
        >
          Cancel
        </button>
      </div>
    </div>
  )
}
