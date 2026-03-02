import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef } from 'react'

import { MessageBubble } from '@/components/message-bubble'
import { ReplyBox } from '@/components/reply-box'
import { avatarColor, avatarInitial, extractEmail, extractName } from '@/lib/avatar'
import { fetchJson, postJson } from '@/lib/api'
import { formatFullDate } from '@/lib/format'
import type { ConversationSummary, ThreadMessage } from '@/lib/types'
import { conversationsAtom, selectedThreadIdAtom, threadMessagesAtom } from '@/store/chat'
import { authAtom } from '@/store/auth'

export function ThreadView() {
  const auth = useAtomValue(authAtom)
  const selectedId = useAtomValue(selectedThreadIdAtom)
  const setSelectedId = useSetAtom(selectedThreadIdAtom)
  const messages = useAtomValue(threadMessagesAtom)
  const setMessages = useSetAtom(threadMessagesAtom)
  const setConversations = useSetAtom(conversationsAtom)
  const bottomRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)

  const loadMessages = useCallback(
    async (threadId: string) => {
      // cancel previous request
      abortRef.current?.abort()
      const controller = new AbortController()
      abortRef.current = controller

      try {
        const data = await fetchJson<ThreadMessage[]>(
          `/conversations/${encodeURIComponent(threadId)}`,
          controller.signal
        )
        if (controller.signal.aborted) return
        setMessages(data)

        // mark as read (fire-and-forget)
        postJson(`/conversations/${encodeURIComponent(threadId)}/read`, {}).catch(
          () => {}
        )

        // refresh conversation list for unread counts
        fetchJson<ConversationSummary[]>('/conversations?limit=50')
          .then((convos) => {
            if (!controller.signal.aborted) setConversations(convos)
          })
          .catch(() => {})
      } catch {
        // aborted or network error
      }
    },
    [setMessages, setConversations]
  )

  useEffect(() => {
    if (!selectedId) {
      setMessages([])
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

  // derive reply recipients
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
    <div className="flex flex-1 flex-col">
      {/* thread header */}
      <div className="flex items-center justify-between border-b border-zinc-200 px-6 py-3 dark:border-zinc-800">
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
      <div className="flex-1 overflow-y-auto px-6 py-4">
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          {messages.map((msg) => {
            const senderEmail = extractEmail(msg.sender)
            const isOwn = senderEmail === myEmail
            const name = extractName(msg.sender)
            const initial = avatarInitial(msg.sender)
            const color = avatarColor(msg.sender)

            return (
              <div
                key={msg.id}
                className={`flex gap-3 ${isOwn ? 'flex-row-reverse' : ''}`}
              >
                {!isOwn && (
                  <div
                    className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-xs font-medium text-white ${color}`}
                  >
                    {initial}
                  </div>
                )}
                <div
                  className={`max-w-[75%] ${isOwn ? 'items-end' : 'items-start'} flex flex-col`}
                >
                  <div className="mb-1 flex items-center gap-2">
                    <span className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
                      {isOwn ? 'You' : name}
                    </span>
                    <span className="text-xs text-zinc-400">
                      {formatFullDate(msg.internal_date)}
                    </span>
                  </div>
                  <MessageBubble
                    uid={msg.uid}
                    textBody={msg.text_body}
                    htmlBody={msg.html_body}
                    attachments={msg.attachments}
                    isOwn={isOwn}
                  />
                </div>
              </div>
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
  )
}
