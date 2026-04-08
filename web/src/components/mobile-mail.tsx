// mobile-mail.tsx — complete mobile email experience
// replaces desktop Chat (MPaneGroup/MPane) with full-screen view switching
// shares data layer (atoms, API) but independent UI

import type { ThreadMessage } from '@/lib/types'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { ArrowLeft, Mail, MessageSquare, Reply } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { MessageBubble } from '@/components/message-bubble'
import { ReplyBox, type ReplyMode } from '@/components/reply-box'
import { SenderAvatar } from '@/components/sender-avatar'
import { fetchJson } from '@/lib/api'
import { extractEmail, extractName } from '@/lib/avatar'
import { formatDate, formatFullDate } from '@/lib/format'
import { authAtom } from '@/store/auth'
import {
  conversationsAtom,
  mobileViewAtom,
  selectedThreadIdAtom,
  threadMessagesAtom,
} from '@/store/chat'

// ─── mobile mail router ─────────────────────────────────────

export function MobileMail() {
  const mobileView = useAtomValue(mobileViewAtom)

  switch (mobileView) {
    case 'conversation':
      return <MobileConversationView />
    case 'reply':
      return <MobileReplyView />
    case 'thread':
      return <MobileThreadView />
    default:
      return null
  }
}

// ─── thread view (full-screen email reader) ──────────────────

function MobileConversationView() {
  const messages = useAtomValue(threadMessagesAtom)
  const auth = useAtomValue(authAtom)
  const myEmail = auth?.address ?? ''
  const setMobileView = useSetAtom(mobileViewAtom)
  const conversations = useAtomValue(conversationsAtom)
  const selectedId = useAtomValue(selectedThreadIdAtom)
  const conversation = conversations.find((c) => c.thread_id === selectedId)
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'instant' })
  }, [messages.length])

  return (
    <div className="flex h-full flex-col">
      {/* header */}
      <div className="border-border flex shrink-0 items-center gap-2 border-b px-3 py-2">
        <button
          className="text-fg-muted hover:text-fg -ml-1 p-1"
          onClick={() => setMobileView('thread')}
        >
          <ArrowLeft className="h-5 w-5" />
        </button>
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-sm font-semibold">Conversation ({messages.length})</h2>
          <p className="text-fg-muted truncate text-xs">
            {conversation?.subject || '(no subject)'}
          </p>
        </div>
      </div>

      {/* message timeline — scrollable */}
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <div className="space-y-3">
          {messages.map((msg) => {
            const name = extractName(msg.sender)
            const isOwn = extractEmail(msg.sender) === myEmail
            const text = msg.text_body || msg.summary || ''
            const preview = text.length > 200 ? text.slice(0, 200) + '...' : text

            return (
              <div className="border-border flex gap-3 border-b pb-3 last:border-b-0" key={msg.id}>
                <SenderAvatar sender={msg.sender} size={28} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span
                      className={`truncate text-xs font-medium ${isOwn ? 'text-accent' : 'text-fg'}`}
                    >
                      {isOwn ? 'Me' : name}
                    </span>
                    <span className="text-fg-muted shrink-0 text-[10px]">
                      {formatDate(msg.internal_date)}
                    </span>
                  </div>
                  <p className="text-fg-muted mt-0.5 text-xs leading-relaxed">{preview}</p>
                </div>
              </div>
            )
          })}
          <div ref={bottomRef} />
        </div>
      </div>

      {/* reply button at bottom */}
      <div className="border-border shrink-0 border-t px-4 py-3">
        <button
          className="bg-accent flex w-full items-center justify-center gap-2 rounded-lg py-2.5 text-sm font-medium text-white active:opacity-90"
          onClick={() => setMobileView('reply')}
        >
          <Reply className="h-4 w-4" />
          Reply
        </button>
      </div>
    </div>
  )
}

// ─── conversation timeline view ──────────────────────────────

function MobileReplyView() {
  const auth = useAtomValue(authAtom)
  const selectedId = useAtomValue(selectedThreadIdAtom)
  const messages = useAtomValue(threadMessagesAtom)
  const setMobileView = useSetAtom(mobileViewAtom)
  const conversations = useAtomValue(conversationsAtom)
  const conversation = conversations.find((c) => c.thread_id === selectedId)

  const [replyMode, setReplyMode] = useState<ReplyMode>('reply')
  const lastMsg = messages[messages.length - 1]
  const subject = conversation?.subject || lastMsg?.subject || '(no subject)'

  if (!selectedId || !lastMsg) {
    setMobileView('thread')
    return null
  }

  const senderEmail = extractEmail(lastMsg.sender)
  const replyRecipients = senderEmail
  // reply-all: include all recipients except self
  const allRecipients = lastMsg.recipients
    ? lastMsg.recipients
        .split(/[,;]\s*/)
        .concat(senderEmail)
        .filter((e) => e !== auth?.address)
        .join(', ')
    : replyRecipients

  return (
    <div className="flex h-full flex-col">
      {/* header */}
      <div className="border-border flex shrink-0 items-center gap-2 border-b px-3 py-2">
        <button
          className="text-fg-muted hover:text-fg -ml-1 p-1"
          onClick={() => setMobileView('thread')}
        >
          <ArrowLeft className="h-5 w-5" />
        </button>
        <h2 className="min-w-0 flex-1 truncate text-sm font-semibold">{subject}</h2>
      </div>

      {/* reply box — takes all remaining space, browser handles keyboard natively */}
      <div className="min-h-0 flex-1 overflow-hidden">
        <ReplyBox
          lastMessageId={lastMsg.message_id ?? ''}
          mode={replyMode}
          onModeChange={setReplyMode}
          onSent={() => setMobileView('thread')}
          originalBody={lastMsg.text_body || ''}
          originalDate={new Date(lastMsg.internal_date * 1000).toISOString()}
          originalFrom={lastMsg.sender}
          originalHtmlBody={lastMsg.html_body || null}
          replyAllRecipients={allRecipients}
          replyRecipients={replyRecipients}
          subject={subject}
          threadId={selectedId}
        />
      </div>
    </div>
  )
}

// ─── full-screen reply view ──────────────────────────────────

function MobileThreadView() {
  const auth = useAtomValue(authAtom)
  const myEmail = auth?.address ?? ''
  const selectedId = useAtomValue(selectedThreadIdAtom)
  const [messages, setMessages] = useAtom(threadMessagesAtom)
  const setMobileView = useSetAtom(mobileViewAtom)
  const conversations = useAtomValue(conversationsAtom)

  const [loading, setLoading] = useState(false)
  const [selectedMsgIdx, setSelectedMsgIdx] = useState<null | number>(null)
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!selectedId) return
    let cancelled = false
    setLoading(true)
    setSelectedMsgIdx(null)
    fetchJson<ThreadMessage[]>(`/conversations/${encodeURIComponent(selectedId)}`)
      .then((data) => {
        if (!cancelled) {
          setMessages(data)
          if (data.length > 0) setSelectedMsgIdx(data.length - 1)
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [selectedId, setMessages])

  useEffect(() => {
    scrollRef.current?.scrollTo(0, 0)
  }, [selectedId])

  const selectedMsg =
    selectedMsgIdx != null ? messages[selectedMsgIdx] : messages[messages.length - 1]
  const conversation = conversations.find((c) => c.thread_id === selectedId)
  const subject = conversation?.subject || selectedMsg?.subject || '(no subject)'

  if (!selectedId) return null

  return (
    <div className="flex h-full flex-col">
      {/* header */}
      <div className="border-border flex shrink-0 items-center gap-2 border-b px-3 py-2">
        <button
          className="text-fg-muted hover:text-fg -ml-1 p-1"
          onClick={() => setMobileView('list')}
        >
          <ArrowLeft className="h-5 w-5" />
        </button>
        <h2 className="min-w-0 flex-1 truncate text-sm font-semibold">{subject}</h2>
        {messages.length > 1 && (
          <button
            className="text-fg-muted hover:text-fg flex items-center gap-1 rounded-md px-2 py-1 text-xs"
            onClick={() => setMobileView('conversation')}
          >
            <MessageSquare className="h-4 w-4" />
            <span>{messages.length}</span>
          </button>
        )}
      </div>

      {/* email body — scrollable */}
      <div className="relative min-h-0 flex-1 overflow-y-auto" ref={scrollRef}>
        {loading ? (
          <div className="flex items-center justify-center py-12">
            <div className="border-border border-t-accent h-6 w-6 animate-spin rounded-full border-2" />
          </div>
        ) : selectedMsg ? (
          <div>
            {/* sender metadata */}
            <div className="px-4 py-3">
              <div className="flex items-center gap-3">
                <SenderAvatar sender={selectedMsg.sender} size={40} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-fg truncate text-sm font-semibold">
                      {extractName(selectedMsg.sender)}
                    </span>
                    <span className="text-fg-muted shrink-0 text-xs">
                      {formatFullDate(selectedMsg.internal_date)}
                    </span>
                  </div>
                  <p className="text-fg-muted truncate text-xs">to {selectedMsg.recipients}</p>
                </div>
              </div>
            </div>

            {/* email content */}
            <div className="px-0">
              <MessageBubble
                attachments={selectedMsg.attachments}
                htmlBody={selectedMsg.html_body}
                isOwn={extractEmail(selectedMsg.sender) === myEmail}
                textBody={selectedMsg.text_body}
                uid={selectedMsg.uid}
              />
            </div>

            {/* conversation hint */}
            {messages.length > 1 && (
              <div className="border-border border-t px-4 py-3">
                <button
                  className="text-accent flex w-full items-center justify-center gap-2 rounded-lg py-2 text-sm font-medium"
                  onClick={() => setMobileView('conversation')}
                >
                  <MessageSquare className="h-4 w-4" />
                  View conversation ({messages.length} messages)
                </button>
              </div>
            )}

            {/* bottom spacer for FAB */}
            <div className="h-20" />
          </div>
        ) : (
          <div className="text-fg-muted flex flex-col items-center justify-center py-12">
            <Mail className="mb-2 h-8 w-8" />
            <p className="text-sm">No message selected</p>
          </div>
        )}

        {/* floating reply button */}
        <button
          className="bg-accent sticky bottom-4 left-full z-10 mr-4 -ml-20 flex h-14 w-14 items-center justify-center rounded-full text-white shadow-lg active:scale-95"
          onClick={() => setMobileView('reply')}
        >
          <Reply className="h-6 w-6" />
        </button>
      </div>
    </div>
  )
}
