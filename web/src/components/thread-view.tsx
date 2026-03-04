import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef, useState } from 'react'

import { AiAnalysisPanel } from '@/components/ai-analysis'
import { CategoryBadge } from '@/components/category-badge'
import { MessageBubble } from '@/components/message-bubble'
import { ReplyBox } from '@/components/reply-box'
import { avatarColor, avatarInitial, extractEmail, extractName } from '@/lib/avatar'
import { fetchJson, postJson } from '@/lib/api'
import { formatFullDate, formatSize } from '@/lib/format'
import type { ConversationSummary, ThreadMessage } from '@/lib/types'
import { categoryFilterAtom, conversationsAtom, searchQueryAtom, selectedThreadIdAtom, threadMessagesAtom } from '@/store/chat'
import { authAtom } from '@/store/auth'

export function ThreadView() {
  const auth = useAtomValue(authAtom)
  const selectedId = useAtomValue(selectedThreadIdAtom)
  const setSelectedId = useSetAtom(selectedThreadIdAtom)
  const messages = useAtomValue(threadMessagesAtom)
  const setMessages = useSetAtom(threadMessagesAtom)
  const setConversations = useSetAtom(conversationsAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const categoryRef = useRef(categoryFilter)
  categoryRef.current = categoryFilter
  const searchQuery = useAtomValue(searchQueryAtom)
  const searchRef = useRef(searchQuery)
  searchRef.current = searchQuery
  const bottomRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const [selectedMsgIdx, setSelectedMsgIdx] = useState<number | null>(null)

  const loadMessages = useCallback(
    async (threadId: string) => {
      abortRef.current?.abort()
      const controller = new AbortController()
      abortRef.current = controller

      try {
        const data = await fetchJson<ThreadMessage[]>(
          `/conversations/${encodeURIComponent(threadId)}`,
          controller.signal,
        )
        if (controller.signal.aborted) return
        setMessages(data)
        // auto-select last message
        if (data.length > 0) setSelectedMsgIdx(data.length - 1)

        postJson(`/conversations/${encodeURIComponent(threadId)}/read`, {}).catch(
          () => {},
        )

        // refresh conversation list preserving search + category filter
        const sq = searchRef.current
        let path = sq
          ? `/conversations/search?q=${encodeURIComponent(sq)}&limit=50`
          : '/conversations?limit=50'
        if (categoryRef.current) {
          path += `&category=${encodeURIComponent(categoryRef.current)}`
        }
        fetchJson<ConversationSummary[]>(path)
          .then((convos) => {
            if (!controller.signal.aborted) setConversations(convos)
          })
          .catch(() => {})
      } catch {
        // aborted or network error
      }
    },
    [setMessages, setConversations],
  )

  useEffect(() => {
    if (!selectedId) {
      setMessages([])
      setSelectedMsgIdx(null)
      return
    }
    loadMessages(selectedId)
    return () => {
      abortRef.current?.abort()
    }
  }, [selectedId, loadMessages, setMessages])

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  if (!selectedId) {
    return (
      <div className="flex flex-1 items-center justify-center text-zinc-400">
        <div className="text-center">
          <svg
            className="mx-auto mb-3 h-12 w-12 text-zinc-300 dark:text-zinc-600"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M8.625 12a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H8.25m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H12m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0h-.375M21 12c0 4.556-4.03 8.25-9 8.25a9.764 9.764 0 01-2.555-.337A5.972 5.972 0 015.41 20.97a5.969 5.969 0 01-.474-.065 4.48 4.48 0 00.978-2.025c.09-.457-.133-.901-.467-1.226C3.93 16.178 3 14.189 3 12c0-4.556 4.03-8.25 9-8.25s9 3.694 9 8.25z"
            />
          </svg>
          <p className="text-sm">Select a conversation</p>
        </div>
      </div>
    )
  }

  const subject = messages[0]?.subject ?? ''
  const lastMsg = messages[messages.length - 1]
  const myEmail = auth?.address ?? ''
  const selectedMsg = selectedMsgIdx !== null ? messages[selectedMsgIdx] : null

  const replyRecipients = lastMsg
    ? (() => {
        const senderEmail = extractEmail(lastMsg.sender)
        const recipientEmails = lastMsg.recipients
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean)
        const all = new Set([senderEmail, ...recipientEmails])
        all.delete(myEmail)
        return [...all].join(', ')
      })()
    : ''

  return (
    <div className="flex flex-1 overflow-hidden">
      {/* left: message list */}
      <div className="flex w-[420px] shrink-0 flex-col border-r border-zinc-200 dark:border-zinc-800">
        {/* thread header */}
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <div className="min-w-0">
            <h2 className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
              {subject || '(no subject)'}
            </h2>
            <p className="truncate text-xs text-zinc-400">
              {messages.length} message{messages.length !== 1 && 's'}
            </p>
          </div>
          <button
            onClick={() => setSelectedId(null)}
            className="shrink-0 text-zinc-400 transition-colors hover:text-zinc-600 dark:hover:text-zinc-300"
          >
            <svg
              className="h-5 w-5"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
            >
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* messages */}
        <div className="flex-1 overflow-y-auto px-4 py-3">
          <div className="flex flex-col gap-3">
            {messages.map((msg, idx) => {
              const senderEmail = extractEmail(msg.sender)
              const isOwn = senderEmail === myEmail
              const name = extractName(msg.sender)
              const initial = avatarInitial(msg.sender)
              const color = avatarColor(msg.sender)
              const isSelected = selectedMsgIdx === idx

              return (
                <button
                  key={msg.id}
                  onClick={() => setSelectedMsgIdx(idx)}
                  className={`flex w-full gap-3 rounded-lg p-2 text-left transition-colors ${
                    isSelected
                      ? 'bg-blue-50 ring-1 ring-blue-200 dark:bg-blue-950/30 dark:ring-blue-800'
                      : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
                  } ${isOwn ? 'flex-row-reverse' : ''}`}
                >
                  {!isOwn && (
                    <div
                      className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-xs font-medium text-white ${color}`}
                    >
                      {initial}
                    </div>
                  )}
                  <div className={`min-w-0 flex-1 ${isOwn ? 'text-right' : ''}`}>
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
                        {isOwn ? 'You' : name}
                      </span>
                      <CategoryBadge category={msg.category} />
                      <span className="text-xs text-zinc-400">
                        {formatFullDate(msg.internal_date)}
                      </span>
                    </div>
                    <p className="mt-0.5 truncate text-xs text-zinc-500 dark:text-zinc-400">
                      {msg.clean_text?.slice(0, 120) || msg.text_body?.slice(0, 120) || msg.subject || '(no content)'}
                    </p>
                  </div>
                </button>
              )
            })}
            <div ref={bottomRef} />
          </div>
        </div>

        {/* reply */}
        <ReplyBox
          threadId={selectedId}
          lastMessageId={lastMsg?.message_id ?? ''}
          recipients={replyRecipients || extractEmail(messages[0]?.sender ?? '')}
          subject={subject}
          onSent={() => loadMessages(selectedId)}
        />
      </div>

      {/* right: selected message detail */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {selectedMsg ? (
          <>
            {/* message header */}
            <div className="border-b border-zinc-200 px-6 py-4 dark:border-zinc-800">
              <div className="flex items-center gap-2">
                <h3 className="text-base font-semibold text-zinc-900 dark:text-zinc-100">
                  {selectedMsg.subject || '(no subject)'}
                </h3>
                <CategoryBadge category={selectedMsg.category} />
                {selectedMsg.ai_analyzed && (
                  <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium ${
                    selectedMsg.risk_score >= 60
                      ? 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
                      : selectedMsg.risk_score >= 40
                        ? 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400'
                        : selectedMsg.risk_score >= 15
                          ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400'
                          : 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
                  }`}>
                    {selectedMsg.risk_score >= 60 ? 'Dangerous' : selectedMsg.risk_score >= 40 ? 'Suspicious' : selectedMsg.risk_score >= 15 ? 'Caution' : 'Safe'}
                    {selectedMsg.risk_score > 0 && ` ${selectedMsg.risk_score}`}
                  </span>
                )}
              </div>
              <div className="mt-2 flex items-center gap-3">
                <div
                  className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-medium text-white ${avatarColor(selectedMsg.sender)}`}
                >
                  {avatarInitial(selectedMsg.sender)}
                </div>
                <div className="min-w-0">
                  <p className="text-sm font-medium text-zinc-900 dark:text-zinc-100">
                    {extractName(selectedMsg.sender)}
                  </p>
                  <p className="text-xs text-zinc-500 dark:text-zinc-400">
                    to {selectedMsg.recipients.slice(0, 80)}
                    {' · '}
                    {formatFullDate(selectedMsg.internal_date)}
                  </p>
                </div>
              </div>
            </div>

            {/* AI analysis */}
            <AiAnalysisPanel message={selectedMsg} />

            {/* message body */}
            <div className="flex-1 overflow-y-auto">
              {/* html render */}
              {selectedMsg.html_body && (
                <div className="border-b border-zinc-200 dark:border-zinc-800">
                  <MessageBubble
                    uid={selectedMsg.uid}
                    textBody={null}
                    htmlBody={selectedMsg.html_body}
                    attachments={[]}
                    isOwn={false}
                  />
                </div>
              )}

              {/* plain text body */}
              <div className="px-6 py-4">
                <div className="mb-2 flex items-center gap-2">
                  <span className="text-xs font-medium uppercase tracking-wide text-zinc-400">
                    {selectedMsg.clean_text ? 'AI Extracted Text' : 'Plain Text'}
                  </span>
                  <div className="h-px flex-1 bg-zinc-200 dark:bg-zinc-700" />
                </div>
                <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-zinc-800 dark:text-zinc-200">
                  {selectedMsg.clean_text || selectedMsg.text_body || '(no text content)'}
                </pre>
              </div>

              {/* attachments */}
              {selectedMsg.attachments.length > 0 && (
                <div className="border-t border-zinc-200 px-6 py-4 dark:border-zinc-800">
                  <div className="mb-2 flex items-center gap-2">
                    <span className="text-xs font-medium uppercase tracking-wide text-zinc-400">
                      Attachments ({selectedMsg.attachments.length})
                    </span>
                    <div className="h-px flex-1 bg-zinc-200 dark:bg-zinc-700" />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    {selectedMsg.attachments.map((att, i) => (
                      <a
                        key={i}
                        href={`/api/mail/messages/${selectedMsg.uid}/attachments/${i}`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex items-center gap-2 rounded-md border border-zinc-200 px-3 py-2 text-sm transition-colors hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
                      >
                        <svg
                          className="h-5 w-5 text-zinc-400"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="1.5"
                        >
                          <path
                            strokeLinecap="round"
                            strokeLinejoin="round"
                            d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z"
                          />
                        </svg>
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-zinc-700 dark:text-zinc-300">
                            {att.filename}
                          </p>
                          <p className="text-xs text-zinc-400">
                            {att.content_type} · {formatSize(att.size)}
                          </p>
                        </div>
                      </a>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </>
        ) : (
          <div className="flex flex-1 items-center justify-center text-zinc-400">
            <p className="text-sm">Select a message to view</p>
          </div>
        )}
      </div>
    </div>
  )
}
