import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { AiAnalysisPanel } from '@/components/ai-analysis'
import { AttachmentPreview } from '@/components/attachment-preview'
import { CategoryBadge } from '@/components/category-badge'
import { MessageBubble } from '@/components/message-bubble'
import { ReplyBox, type ReplyMode } from '@/components/reply-box'
import { avatarColor, avatarInitial, extractEmail, extractName } from '@/lib/avatar'
import { deleteJson, fetchJson, postJson } from '@/lib/api'
import { getToken } from '@/store/auth'
import { formatFullDate } from '@/lib/format'
import type { ConversationSummary, ThreadMessage } from '@/lib/types'
import { categoryFilterAtom, conversationsAtom, searchQueryAtom, selectedDomainsAtom, selectedThreadIdAtom, threadMessagesAtom } from '@/store/chat'
import { authAtom } from '@/store/auth'

// source message used as the forward origin (may differ from lastMsg)
type ForwardSource = {
  sender: string
  date: string
  subject: string
  body: string
  messageId: string
}

export function ThreadView({ onBack }: { onBack?: () => void }) {
  const auth = useAtomValue(authAtom)
  const selectedId = useAtomValue(selectedThreadIdAtom)
  const setSelectedId = useSetAtom(selectedThreadIdAtom)
  const messages = useAtomValue(threadMessagesAtom)
  const setMessages = useSetAtom(threadMessagesAtom)
  const conversations = useAtomValue(conversationsAtom)
  const conversationsRef = useRef(conversations)
  conversationsRef.current = conversations
  const setConversations = useSetAtom(conversationsAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const categoryRef = useRef(categoryFilter)
  categoryRef.current = categoryFilter
  const searchQuery = useAtomValue(searchQueryAtom)
  const searchRef = useRef(searchQuery)
  searchRef.current = searchQuery
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const domainsRef = useRef(selectedDomains)
  domainsRef.current = selectedDomains
  const bottomRef = useRef<HTMLDivElement>(null)
  const replyBoxRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const [selectedMsgIdx, setSelectedMsgIdx] = useState<number | null>(null)
  const [isRead, setIsRead] = useState(true)
  const [isFlagged, setIsFlagged] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)
  const [replyMode, setReplyMode] = useState<ReplyMode>('reply')
  const [forwardSource, setForwardSource] = useState<ForwardSource | null>(null)
  const [loadingThread, setLoadingThread] = useState(false)
  const [mobileView, setMobileView] = useState<'list' | 'detail'>('list')

  const loadMessages = useCallback(
    async (threadId: string) => {
      abortRef.current?.abort()
      const controller = new AbortController()
      abortRef.current = controller
      setLoadingThread(true)

      try {
        const doms = domainsRef.current
        const domainsParam = doms.length > 0 ? `?domains=${encodeURIComponent(doms.join(','))}` : ''
        const data = await fetchJson<ThreadMessage[]>(
          `/conversations/${encodeURIComponent(threadId)}${domainsParam}`,
          controller.signal,
        )
        if (controller.signal.aborted) return
        setMessages(data)
        // auto-select last message
        if (data.length > 0) setSelectedMsgIdx(data.length - 1)

        postJson(`/conversations/${encodeURIComponent(threadId)}/read`, {}).catch(
          () => {},
        )
        setIsRead(true)

        // refresh conversation list preserving search + category + domain filter
        const sq = searchRef.current
        let path = sq
          ? `/conversations/search?q=${encodeURIComponent(sq)}&limit=50`
          : '/conversations?limit=50'
        if (categoryRef.current) {
          path += `&category=${encodeURIComponent(categoryRef.current)}`
        }
        if (doms.length > 0) {
          path += `&domains=${encodeURIComponent(doms.join(','))}`
        }
        fetchJson<ConversationSummary[]>(path)
          .then((convos) => {
            if (!controller.signal.aborted) setConversations(convos)
          })
          .catch(() => {})
      } catch (err) {
        if (!controller.signal.aborted) {
          toast.error(err instanceof Error ? err.message : 'Failed to load messages')
        }
      } finally {
        setLoadingThread(false)
      }
    },
    [setMessages, setConversations],
  )

  const refreshConversations = useCallback(async () => {
    const sq = searchRef.current
    const doms = domainsRef.current
    let path = sq
      ? `/conversations/search?q=${encodeURIComponent(sq)}&limit=50`
      : '/conversations?limit=50'
    if (categoryRef.current) {
      path += `&category=${encodeURIComponent(categoryRef.current)}`
    }
    if (doms.length > 0) {
      path += `&domains=${encodeURIComponent(doms.join(','))}`
    }
    try {
      const convos = await fetchJson<ConversationSummary[]>(path)
      setConversations(convos)
    } catch {
      // ignore refresh errors
    }
  }, [setConversations])

  const handleMarkUnread = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/unread`, {})
      setIsRead(false)
      setConversations((prev) =>
        prev.map((c) =>
          c.thread_id === selectedId ? { ...c, unread_count: c.unread_count + 1 } : c,
        ),
      )
      toast.success('Marked as unread')
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to mark as unread')
    }
  }, [selectedId, setConversations])

  const handleMarkRead = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/read`, {})
      setIsRead(true)
      setConversations((prev) =>
        prev.map((c) =>
          c.thread_id === selectedId ? { ...c, unread_count: 0 } : c,
        ),
      )
      toast.success('Marked as read')
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to mark as read')
    }
  }, [selectedId, setConversations])

  const handleStar = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/star`, {})
      setIsFlagged(true)
      setConversations((prev) =>
        prev.map((c) =>
          c.thread_id === selectedId ? { ...c, flagged: true } : c,
        ),
      )
      toast.success('Starred')
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to star')
    }
  }, [selectedId, setConversations])

  const handleUnstar = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/unstar`, {})
      setIsFlagged(false)
      setConversations((prev) =>
        prev.map((c) =>
          c.thread_id === selectedId ? { ...c, flagged: false } : c,
        ),
      )
      toast.success('Unstarred')
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to unstar')
    }
  }, [selectedId, setConversations])

  const handleDelete = useCallback(async () => {
    if (!selectedId) return
    try {
      await deleteJson(`/conversations/${encodeURIComponent(selectedId)}`)
      toast.success('Conversation deleted')
      setSelectedId(null)
      setMessages([])
      setShowDeleteConfirm(false)
      await refreshConversations()
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete conversation')
      setShowDeleteConfirm(false)
    }
  }, [selectedId, setSelectedId, setMessages, refreshConversations])

  const handlePrint = useCallback((msg: ThreadMessage) => {
    const w = window.open('', '_blank')
    if (!w) return
    const escHtml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    const body = msg.html_body
      ? msg.html_body
      : `<pre style="white-space:pre-wrap;word-break:break-word;font-family:sans-serif;font-size:14px;line-height:1.6">${escHtml(msg.clean_text || msg.text_body || '(no content)')}</pre>`
    w.document.write(`<!DOCTYPE html><html><head><meta charset="utf-8"><title>${escHtml(msg.subject || '(no subject)')}</title><style>body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;padding:2rem;max-width:800px;margin:0 auto;color:#1a1a1a}table{border-collapse:collapse;margin-bottom:1.5rem;width:100%}td{padding:4px 8px;vertical-align:top;font-size:14px}td:first-child{font-weight:600;white-space:nowrap;color:#555;width:80px}hr{border:none;border-top:1px solid #ddd;margin:1rem 0}.body{font-size:14px;line-height:1.6}img{max-width:100%}@media print{body{padding:0}}</style></head><body><table><tr><td>From</td><td>${escHtml(msg.sender)}</td></tr><tr><td>To</td><td>${escHtml(msg.recipients)}</td></tr><tr><td>Date</td><td>${escHtml(formatFullDate(msg.internal_date))}</td></tr><tr><td>Subject</td><td>${escHtml(msg.subject || '(no subject)')}</td></tr></table><hr><div class="body">${body}</div></body></html>`)
    w.document.close()
    w.onload = () => w.print()
  }, [])

  const handleDownloadEml = useCallback(async (uid: number, subject: string) => {
    try {
      const token = getToken()
      const headers: Record<string, string> = {}
      if (token) headers['Authorization'] = `Bearer ${token}`
      const res = await fetch(`/api/mail/messages/${uid}/raw`, { headers })
      if (!res.ok) {
        toast.error('Failed to download .eml file')
        return
      }
      const blob = await res.blob()
      const safeName = subject.replace(/[^a-zA-Z0-9\u4e00-\u9fff\u3040-\u30ff _-]/g, '_').trim()
      const filename = safeName ? `${safeName}.eml` : `message-${uid}.eml`
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = filename
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
    } catch {
      toast.error('Failed to download .eml file')
    }
  }, [])

  // trigger forward mode for a specific message, pre-filling its content into ReplyBox
  const handleForwardMsg = useCallback((msg: ThreadMessage) => {
    setForwardSource({
      sender: msg.sender,
      date: formatFullDate(msg.internal_date),
      subject: msg.subject || '',
      body: msg.clean_text || msg.text_body || '',
      messageId: msg.message_id,
    })
    setReplyMode('forward')
    // on mobile: switch to list panel so the reply box is visible
    setMobileView('list')
    // scroll reply box into view after state flush
    requestAnimationFrame(() => {
      replyBoxRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
    })
  }, [])

  useEffect(() => {
    if (!selectedId) {
      setMessages([])
      setSelectedMsgIdx(null)
      setShowDeleteConfirm(false)
      setForwardSource(null)
      setMobileView('list')
      return
    }
    setMobileView('list')
    setForwardSource(null)
    setReplyMode('reply')
    // optimistically set read/flagged state from conversation list before loading
    const existing = conversationsRef.current.find((c) => c.thread_id === selectedId)
    setIsRead(!existing || existing.unread_count === 0)
    setIsFlagged(existing?.flagged ?? false)
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
      <div className="flex flex-1 flex-col">
        {/* mobile back button when no conversation selected */}
        {onBack && (
          <div className="flex items-center border-b border-zinc-200 px-4 py-3 dark:border-zinc-800 md:hidden">
            <button
              onClick={onBack}
              className="flex items-center gap-1.5 text-sm text-zinc-500 transition-colors hover:text-zinc-800 dark:hover:text-zinc-200"
            >
              <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
              </svg>
              Back
            </button>
          </div>
        )}
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
      </div>
    )
  }

  const subject = messages[0]?.subject ?? ''
  const lastMsg = messages[messages.length - 1]
  const myEmail = auth?.address ?? ''
  const selectedMsg = selectedMsgIdx !== null ? messages[selectedMsgIdx] : null

  // reply: only the sender of the last message
  const replyRecipients = lastMsg ? extractEmail(lastMsg.sender) : ''

  // reply-all: sender + all recipients, excluding self
  const replyAllRecipients = lastMsg
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

  // body used for forward quoting: prefer clean_text, fall back to text_body
  const lastMsgBody = lastMsg?.clean_text || lastMsg?.text_body || ''
  const lastMsgDate = lastMsg ? formatFullDate(lastMsg.internal_date) : ''

  // when a specific message is being forwarded, use its data; otherwise fall back to lastMsg
  const fwdOriginalFrom = forwardSource?.sender ?? lastMsg?.sender ?? ''
  const fwdOriginalDate = forwardSource?.date ?? lastMsgDate
  const fwdSubject = forwardSource?.subject ?? subject
  const fwdOriginalBody = forwardSource?.body ?? lastMsgBody
  const fwdLastMessageId = forwardSource?.messageId ?? lastMsg?.message_id ?? ''

  return (
    <div className="flex flex-1 overflow-hidden">
      {/* left: message list (hidden on mobile when viewing detail) */}
      <div className={`${mobileView === 'detail' ? 'hidden' : 'flex'} w-full shrink-0 flex-col border-r border-zinc-200 dark:border-zinc-800 md:flex md:w-[420px]`}>
        {/* thread header */}
        <div className="flex items-center gap-2 border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          {/* mobile back button */}
          {onBack && (
            <button
              onClick={onBack}
              className="shrink-0 text-zinc-400 transition-colors hover:text-zinc-600 dark:hover:text-zinc-300 md:hidden"
              title="Back to list"
            >
              <svg className="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
              </svg>
            </button>
          )}
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
              {subject || '(no subject)'}
            </h2>
            <p className="truncate text-xs text-zinc-400">
              {messages.length} message{messages.length !== 1 && 's'}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {/* toggle read/unread */}
            <button
              onClick={isRead ? handleMarkUnread : handleMarkRead}
              className="rounded p-1 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
              title={isRead ? 'Mark as unread' : 'Mark as read'}
            >
              {isRead ? (
                <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M21.75 9v.906a2.25 2.25 0 01-1.183 1.981l-6.478 3.488M2.25 9v.906a2.25 2.25 0 001.183 1.981l6.478 3.488m8.839 2.51-4.66-2.51m0 0-1.023-.55a2.25 2.25 0 00-2.134 0l-1.022.55m0 0-4.661 2.51m16.5 1.615a2.25 2.25 0 01-2.25 2.25h-15a2.25 2.25 0 01-2.25-2.25V8.844a2.25 2.25 0 011.183-1.981l7.5-4.039a2.25 2.25 0 012.134 0l7.5 4.039a2.25 2.25 0 011.183 1.98V19.5z" />
                </svg>
              ) : (
                <svg className="h-4 w-4" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M1.5 8.67v8.58a3 3 0 003 3h15a3 3 0 003-3V8.67l-8.928 5.493a3 3 0 01-3.144 0L1.5 8.67z" />
                  <path d="M22.5 6.908V6.75a3 3 0 00-3-3h-15a3 3 0 00-3 3v.158l9.714 5.978a1.5 1.5 0 001.572 0L22.5 6.908z" />
                </svg>
              )}
            </button>
            {/* star/unstar */}
            <button
              onClick={isFlagged ? handleUnstar : handleStar}
              className={`rounded p-1 transition-colors ${
                isFlagged
                  ? 'text-yellow-400 hover:bg-yellow-50 hover:text-yellow-500 dark:hover:bg-yellow-900/20'
                  : 'text-zinc-400 hover:bg-zinc-100 hover:text-yellow-400 dark:hover:bg-zinc-800'
              }`}
              title={isFlagged ? 'Unstar' : 'Star'}
            >
              <svg className="h-4 w-4" viewBox="0 0 24 24" fill={isFlagged ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="1.5">
                <path strokeLinecap="round" strokeLinejoin="round" d="M11.48 3.499a.562.562 0 011.04 0l2.125 5.111a.563.563 0 00.475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 00-.182.557l1.285 5.385a.562.562 0 01-.84.61l-4.725-2.885a.563.563 0 00-.586 0L6.982 20.54a.562.562 0 01-.84-.61l1.285-5.386a.562.562 0 00-.182-.557l-4.204-3.602a.563.563 0 01.321-.988l5.518-.442a.563.563 0 00.475-.345L11.48 3.5z" />
              </svg>
            </button>
            {/* delete */}
            <button
              onClick={() => setShowDeleteConfirm(true)}
              className="rounded p-1 text-zinc-400 transition-colors hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-900/20 dark:hover:text-red-400"
              title="Delete conversation"
            >
              <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
              </svg>
            </button>
            {/* close */}
            <button
              onClick={() => setSelectedId(null)}
              aria-label="Close conversation"
              className="rounded p-1 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
            >
              <svg
                className="h-5 w-5"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                aria-hidden="true"
              >
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>
        </div>

        {/* delete confirmation dialog */}
        {showDeleteConfirm && (
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm" role="dialog" aria-modal="true" aria-label="Delete confirmation">
            <div className="mx-4 w-full max-w-sm rounded-xl border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-700 dark:bg-zinc-900">
              <h3 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                Delete conversation?
              </h3>
              <p className="mt-1.5 text-sm text-zinc-500 dark:text-zinc-400">
                This will permanently delete all messages in this thread. This action cannot be undone.
              </p>
              <div className="mt-4 flex justify-end gap-2">
                <button
                  onClick={() => setShowDeleteConfirm(false)}
                  className="rounded-lg border border-zinc-200 px-3 py-1.5 text-sm text-zinc-600 transition-colors hover:bg-zinc-50 dark:border-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-800"
                >
                  Cancel
                </button>
                <button
                  onClick={handleDelete}
                  className="rounded-lg bg-red-600 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-red-700"
                >
                  Delete
                </button>
              </div>
            </div>
          </div>
        )}

        {/* messages */}
        <div className="flex-1 overflow-y-auto px-4 py-3">
          {loadingThread && messages.length === 0 && (
            <div className="animate-pulse">
              {Array.from({ length: 5 }).map((_, i) => (
                <div key={i} className="flex gap-3 rounded-lg p-2">
                  <div className="h-8 w-8 shrink-0 rounded-full bg-zinc-200 dark:bg-zinc-700" />
                  <div className="min-w-0 flex-1 space-y-2">
                    <div className="flex items-center gap-2">
                      <div className="h-3 w-20 rounded bg-zinc-200 dark:bg-zinc-700" />
                      <div className="h-3 w-28 rounded bg-zinc-200 dark:bg-zinc-700" />
                    </div>
                    <div className="h-3 w-3/4 rounded bg-zinc-200 dark:bg-zinc-700" />
                  </div>
                </div>
              ))}
            </div>
          )}
          <div className="flex flex-col gap-3">
            {messages.map((msg, idx) => {
              const senderEmail = extractEmail(msg.sender)
              const isOwn = senderEmail === myEmail
              const name = extractName(msg.sender)
              const initial = avatarInitial(msg.sender)
              const color = avatarColor(msg.sender)
              const isSelected = selectedMsgIdx === idx

              return (
                <div key={msg.id} className="group relative">
                  <button
                    onClick={() => {
                      setSelectedMsgIdx(idx)
                      setMobileView('detail')
                    }}
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
                  {/* forward button revealed on card hover */}
                  <button
                    onClick={() => handleForwardMsg(msg)}
                    className="absolute right-2 top-2 hidden rounded p-1 text-zinc-400 transition-colors hover:bg-zinc-200 hover:text-zinc-600 group-hover:flex dark:hover:bg-zinc-700 dark:hover:text-zinc-300"
                    title="Forward this message"
                  >
                    <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M3 10h10a8 8 0 018 8v2M3 10l6 6m-6-6l6-6" />
                    </svg>
                  </button>
                </div>
              )
            })}
            <div ref={bottomRef} />
          </div>
        </div>

        {/* reply / reply-all / forward */}
        <div ref={replyBoxRef}>
          <ReplyBox
            threadId={selectedId}
            lastMessageId={fwdLastMessageId}
            replyRecipients={replyRecipients || extractEmail(messages[0]?.sender ?? '')}
            replyAllRecipients={replyAllRecipients || extractEmail(messages[0]?.sender ?? '')}
            subject={fwdSubject}
            originalFrom={fwdOriginalFrom}
            originalDate={fwdOriginalDate}
            originalBody={fwdOriginalBody}
            mode={replyMode}
            onModeChange={(m) => {
              setReplyMode(m)
              // clear per-message forward source when user manually changes mode
              if (m !== 'forward') setForwardSource(null)
            }}
            onSent={() => {
              setForwardSource(null)
              loadMessages(selectedId)
            }}
          />
        </div>
      </div>

      {/* right: selected message detail (shown on mobile only in detail view) */}
      <div className={`${mobileView === 'detail' ? 'flex' : 'hidden'} flex-1 flex-col overflow-hidden md:flex`}>
        {selectedMsg ? (
          <>
            {/* message header */}
            <div className="border-b border-zinc-200 px-6 py-4 dark:border-zinc-800">
              {/* mobile back to message list */}
              <button
                onClick={() => setMobileView('list')}
                className="mb-2 flex items-center gap-1.5 text-sm text-zinc-500 transition-colors hover:text-zinc-800 dark:hover:text-zinc-200 md:hidden"
              >
                <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
                </svg>
                Back to messages
              </button>
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
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium text-zinc-900 dark:text-zinc-100">
                    {extractName(selectedMsg.sender)}
                  </p>
                  <p className="text-xs text-zinc-500 dark:text-zinc-400">
                    to {selectedMsg.recipients.slice(0, 80)}
                    {' · '}
                    {formatFullDate(selectedMsg.internal_date)}
                  </p>
                </div>
                {/* forward this specific message */}
                <button
                  onClick={() => handleForwardMsg(selectedMsg)}
                  className="shrink-0 rounded p-1.5 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
                  title="Forward"
                >
                  <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M15 15l6-6m0 0l-6-6m6 6H9a6 6 0 000 12h3" />
                  </svg>
                </button>
                <button
                  onClick={() => handlePrint(selectedMsg)}
                  className="shrink-0 rounded p-1.5 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
                  title="Print"
                >
                  <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6.72 13.829c-.24.03-.48.062-.72.096m.72-.096a42.415 42.415 0 0110.56 0m-10.56 0L6.34 18m10.94-4.171c.24.03.48.062.72.096m-.72-.096L17.66 18m0 0l.229 2.523a1.125 1.125 0 01-1.12 1.227H7.231c-.662 0-1.18-.568-1.12-1.227L6.34 18m11.318 0h1.091A2.25 2.25 0 0021 15.75V9.456c0-1.081-.768-2.015-1.837-2.175a48.055 48.055 0 00-1.913-.247M6.34 18H5.25A2.25 2.25 0 013 15.75V9.456c0-1.081.768-2.015 1.837-2.175a48.041 48.041 0 011.913-.247m10.5 0a48.536 48.536 0 00-10.5 0m10.5 0V3.375c0-.621-.504-1.125-1.125-1.125h-8.25c-.621 0-1.125.504-1.125 1.125v3.659M18.25 7.28H5.75" />
                  </svg>
                </button>
                <button
                  onClick={() => handleDownloadEml(selectedMsg.uid, selectedMsg.subject)}
                  className="shrink-0 rounded p-1.5 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
                  title="Download .eml"
                >
                  <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                  </svg>
                </button>
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
              <AttachmentPreview attachments={selectedMsg.attachments} uid={selectedMsg.uid} />
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
