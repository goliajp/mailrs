import { useAtomValue, useSetAtom } from 'jotai'
import { ArrowLeft, Download, Forward, Mail, MailOpen, MoreVertical, Paperclip, Printer, Star, Trash2, X } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { AiAnalysisPanel } from '@/components/ai-analysis'
import { AttachmentPreview } from '@/components/attachment-preview'
import { ActionBadge, CategoryBadge, ImportanceBadge, IntentBadge } from '@/components/category-badge'
import { Copyable } from '@/components/copy-button'
import { StructuredDataCard } from '@/components/structured-data-card'
import { MessageBubble } from '@/components/message-bubble'
import { ReplyBox, type ReplyMode } from '@/components/reply-box'
import DOMPurify from 'dompurify'
import { avatarColor, avatarInitial, extractEmail, extractName } from '@/lib/avatar'
import { deleteJson, fetchJson, postJson, recordFeedback, type FeedbackAction } from '@/lib/api'
import { getToken } from '@/store/auth'
import { formatDate, formatFullDate } from '@/lib/format'
import type { ConversationSummary, ThreadMessage } from '@/lib/types'
import { categoryFilterAtom, conversationsAtom, crossAccountReadAtom, searchQueryAtom, selectedDomainsAtom, selectedThreadIdAtom, threadMessagesAtom } from '@/store/chat'
import { authAtom } from '@/store/auth'

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
  const crossAccountRead = useAtomValue(crossAccountReadAtom)
  const crossAccountReadRef = useRef(crossAccountRead)
  crossAccountReadRef.current = crossAccountRead
  const bottomRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const [selectedMsgIdx, setSelectedMsgIdx] = useState<number | null>(null)
  const [isRead, setIsRead] = useState(true)
  const [isFlagged, setIsFlagged] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)
  const [replyMode, setReplyMode] = useState<ReplyMode>('reply')
  const [forwardSource, setForwardSource] = useState<ForwardSource | null>(null)
  const [loadingThread, setLoadingThread] = useState(false)
  const [expandedBubbles, setExpandedBubbles] = useState<Set<number>>(new Set())

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
        if (data.length > 0) setSelectedMsgIdx(data.length - 1)

        const crossAll = crossAccountReadRef.current
        const readParam = crossAll && doms.length > 0
          ? `?domains=${encodeURIComponent(doms.join(','))}`
          : ''
        postJson(`/conversations/${encodeURIComponent(threadId)}/read${readParam}`, {}).catch(() => {})
        setIsRead(true)

        const sq = searchRef.current
        const cat = categoryRef.current
        let path = sq
          ? `/conversations/search?q=${encodeURIComponent(sq)}&limit=50`
          : '/conversations?limit=50'
        if (cat) path += `&category=${encodeURIComponent(cat)}`
        if (doms.length > 0) path += `&domains=${encodeURIComponent(doms.join(','))}`
        fetchJson<ConversationSummary[]>(path)
          .then((convos) => { if (!controller.signal.aborted) setConversations(convos) })
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
    if (categoryRef.current) path += `&category=${encodeURIComponent(categoryRef.current)}`
    if (doms.length > 0) path += `&domains=${encodeURIComponent(doms.join(','))}`
    try {
      const convos = await fetchJson<ConversationSummary[]>(path)
      setConversations(convos)
    } catch { /* ignore */ }
  }, [setConversations])

  const handleMarkUnread = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/unread`, {})
      setIsRead(false)
      setConversations((prev) => prev.map((c) => c.thread_id === selectedId ? { ...c, unread_count: c.unread_count + 1 } : c))
      toast.success('Marked as unread')
    } catch (err) { toast.error(err instanceof Error ? err.message : 'Failed') }
  }, [selectedId, setConversations])

  const handleMarkRead = useCallback(async () => {
    if (!selectedId) return
    try {
      const doms = domainsRef.current
      const crossAll = crossAccountReadRef.current
      const domainsParam = crossAll && doms.length > 0 ? `?domains=${encodeURIComponent(doms.join(','))}` : ''
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/read${domainsParam}`, {})
      setIsRead(true)
      setConversations((prev) => prev.map((c) => c.thread_id === selectedId ? { ...c, unread_count: 0 } : c))
      toast.success('Marked as read')
    } catch (err) { toast.error(err instanceof Error ? err.message : 'Failed') }
  }, [selectedId, setConversations])

  const handleStar = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/star`, {})
      setIsFlagged(true)
      setConversations((prev) => prev.map((c) => c.thread_id === selectedId ? { ...c, flagged: true } : c))
    } catch (err) { toast.error(err instanceof Error ? err.message : 'Failed') }
  }, [selectedId, setConversations])

  const handleUnstar = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/unstar`, {})
      setIsFlagged(false)
      setConversations((prev) => prev.map((c) => c.thread_id === selectedId ? { ...c, flagged: false } : c))
    } catch (err) { toast.error(err instanceof Error ? err.message : 'Failed') }
  }, [selectedId, setConversations])

  const handleDelete = useCallback(async () => {
    if (!selectedId) return
    try {
      await deleteJson(`/conversations/${encodeURIComponent(selectedId)}`)
      toast.success('Deleted')
      setSelectedId(null)
      setMessages([])
      setShowDeleteConfirm(false)
      await refreshConversations()
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed')
      setShowDeleteConfirm(false)
    }
  }, [selectedId, setSelectedId, setMessages, refreshConversations])

  const handlePrint = useCallback((msg: ThreadMessage) => {
    const w = window.open('', '_blank')
    if (!w) return
    const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    const body = msg.html_body
      ? DOMPurify.sanitize(msg.html_body)
      : `<pre style="white-space:pre-wrap;word-break:break-word;font-family:sans-serif;font-size:14px;line-height:1.6">${esc(msg.clean_text || msg.text_body || '')}</pre>`
    w.document.write(`<!DOCTYPE html><html><head><meta charset="utf-8"><title>${esc(msg.subject || '')}</title><style>body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;padding:2rem;max-width:800px;margin:0 auto}table{border-collapse:collapse;width:100%;margin-bottom:1.5rem}td{padding:4px 8px;font-size:14px}td:first-child{font-weight:600;white-space:nowrap;color:#555;width:80px}hr{border:none;border-top:1px solid #ddd;margin:1rem 0}img{max-width:100%}@media print{body{padding:0}}</style></head><body><table><tr><td>From</td><td>${esc(msg.sender)}</td></tr><tr><td>To</td><td>${esc(msg.recipients)}</td></tr><tr><td>Date</td><td>${esc(formatFullDate(msg.internal_date))}</td></tr><tr><td>Subject</td><td>${esc(msg.subject || '')}</td></tr></table><hr><div>${body}</div></body></html>`)
    w.document.close()
    w.onload = () => w.print()
  }, [])

  const handleDownloadEml = useCallback(async (uid: number, subject: string) => {
    try {
      const token = getToken()
      const headers: Record<string, string> = {}
      if (token) headers['Authorization'] = `Bearer ${token}`
      const res = await fetch(`/api/mail/messages/${uid}/raw`, { headers })
      if (!res.ok) { toast.error('Download failed'); return }
      const blob = await res.blob()
      const safeName = subject.replace(/[^a-zA-Z0-9\u4e00-\u9fff\u3040-\u30ff _-]/g, '_').trim()
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = safeName ? `${safeName}.eml` : `message-${uid}.eml`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
    } catch { toast.error('Download failed') }
  }, [])

  const handleForwardMsg = useCallback((msg: ThreadMessage) => {
    setForwardSource({
      sender: msg.sender,
      date: formatFullDate(msg.internal_date),
      subject: msg.subject || '',
      body: msg.clean_text || msg.text_body || '',
      messageId: msg.message_id,
    })
    setReplyMode('forward')
  }, [])

  useEffect(() => {
    if (!selectedId) {
      setMessages([])
      setSelectedMsgIdx(null)
      setShowDeleteConfirm(false)
      setForwardSource(null)
      setExpandedBubbles(new Set())
      return
    }
    setForwardSource(null)
    setReplyMode('reply')
    setExpandedBubbles(new Set())
    const existing = conversationsRef.current.find((c) => c.thread_id === selectedId)
    setIsRead(!existing || existing.unread_count === 0)
    setIsFlagged(existing?.flagged ?? false)
    loadMessages(selectedId)
    return () => { abortRef.current?.abort() }
  }, [selectedId, loadMessages, setMessages])

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  // empty state
  if (!selectedId) {
    return (
      <div className="flex flex-1 flex-col">
        {onBack && (
          <div className="flex items-center border-b border-zinc-200 px-4 py-3 dark:border-zinc-800 md:hidden">
            <button onClick={onBack} className="flex items-center gap-1.5 text-sm text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200">
              <ArrowLeft className="h-4 w-4" />
              Back
            </button>
          </div>
        )}
        <div className="flex flex-1 items-center justify-center text-zinc-400">
          <div className="text-center">
            <Mail className="mx-auto mb-3 h-12 w-12 text-zinc-300 dark:text-zinc-600" strokeWidth={1} />
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

  const replyRecipients = lastMsg ? extractEmail(lastMsg.sender) : ''
  const replyAllRecipients = lastMsg
    ? (() => {
        const senderEmail = extractEmail(lastMsg.sender)
        const recipientEmails = lastMsg.recipients.split(',').map((s) => s.trim()).filter(Boolean)
        const all = new Set([senderEmail, ...recipientEmails])
        all.delete(myEmail)
        return [...all].join(', ')
      })()
    : ''

  const lastMsgBody = lastMsg?.clean_text || lastMsg?.text_body || ''
  const lastMsgDate = lastMsg ? formatFullDate(lastMsg.internal_date) : ''
  const fwdOriginalFrom = forwardSource?.sender ?? lastMsg?.sender ?? ''
  const fwdOriginalDate = forwardSource?.date ?? lastMsgDate
  const fwdSubject = forwardSource?.subject ?? subject
  const fwdOriginalBody = forwardSource?.body ?? lastMsgBody
  const fwdLastMessageId = forwardSource?.messageId ?? lastMsg?.message_id ?? ''

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* header bar */}
      <div className="flex shrink-0 select-none items-center gap-2 border-b border-zinc-200 px-4 py-2 dark:border-zinc-800">
        {onBack && (
          <button onClick={onBack} className="shrink-0 rounded-md p-1 text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300 md:hidden" title="Back">
            <ArrowLeft className="h-5 w-5" />
          </button>
        )}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h2 className="select-text truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">{subject || '(no subject)'}</h2>
            <CategoryBadge category={messages[0]?.category} />
            <ImportanceBadge level={messages[0]?.importance_level} />
            {messages.some((m) => m.requires_action) && <ActionBadge />}
            <span className="text-xs text-zinc-400">{messages.length} message{messages.length !== 1 && 's'}</span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          <HdrBtn onClick={isRead ? handleMarkUnread : handleMarkRead} title={isRead ? 'Mark unread' : 'Mark read'}>
            {isRead ? (
              <MailOpen className="h-4 w-4" />
            ) : (
              <Mail className="h-4 w-4" />
            )}
          </HdrBtn>
          <HdrBtn onClick={isFlagged ? handleUnstar : handleStar} title={isFlagged ? 'Unstar' : 'Star'} className={isFlagged ? 'text-yellow-400 hover:text-yellow-500' : undefined}>
            <Star className="h-4 w-4" fill={isFlagged ? 'currentColor' : 'none'} />
          </HdrBtn>
          <HdrBtn onClick={() => setShowDeleteConfirm(true)} title="Delete" className="hover:text-red-500">
            <Trash2 className="h-4 w-4" />
          </HdrBtn>
          <HdrBtn onClick={() => setSelectedId(null)} title="Close">
            <X className="h-4 w-4" />
          </HdrBtn>
        </div>
      </div>

      {/* delete dialog */}
      {showDeleteConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" role="dialog" aria-modal="true">
          <div className="mx-4 w-full max-w-sm rounded border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-700 dark:bg-zinc-900">
            <h3 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Delete conversation?</h3>
            <p className="mt-1.5 text-sm text-zinc-500 dark:text-zinc-400">This will permanently delete all messages.</p>
            <div className="mt-4 flex justify-end gap-2">
              <button onClick={() => setShowDeleteConfirm(false)} className="rounded border border-zinc-200 px-3 py-1.5 text-sm text-zinc-600 hover:bg-zinc-50 dark:border-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-800">Cancel</button>
              <button onClick={handleDelete} className="rounded bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-700">Delete</button>
            </div>
          </div>
        </div>
      )}

      {/* main content: two columns */}
      <div className="flex flex-1 overflow-hidden">
        {/* column 1: raw email (current message) */}
        <div className="flex w-1/2 flex-col overflow-hidden border-r border-zinc-200 dark:border-zinc-800">
          {selectedMsg ? (
            <>
              {/* email header */}
              <div className="shrink-0 border-b border-zinc-200 px-5 py-3 dark:border-zinc-800">
                <h3 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                  {selectedMsg.subject || '(no subject)'}
                </h3>
                <div className="mt-2 flex items-center gap-3">
                  <div className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-xs font-medium text-white ${avatarColor(selectedMsg.sender)}`}>
                    {avatarInitial(selectedMsg.sender)}
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="select-text text-sm font-medium text-zinc-900 dark:text-zinc-100">
                      {extractName(selectedMsg.sender)}{' '}
                      <Copyable value={extractEmail(selectedMsg.sender)}>
                        <span className="font-normal text-zinc-500 dark:text-zinc-400">&lt;{extractEmail(selectedMsg.sender)}&gt;</span>
                      </Copyable>
                    </p>
                    <p className="select-text truncate text-xs text-zinc-500 dark:text-zinc-400">
                      to <Copyable value={selectedMsg.recipients.slice(0, 80)}>{selectedMsg.recipients.slice(0, 80)}</Copyable> · {formatFullDate(selectedMsg.internal_date)}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-0.5">
                    {selectedMsg.requires_action && <ActionBadge />}
                    <IntentBadge intent={selectedMsg.sender_intent} />
                    {selectedMsg.action_deadline && (
                      <span className="rounded bg-orange-100 px-2 py-0.5 text-[11px] font-medium text-orange-700 dark:bg-orange-900/30 dark:text-orange-400">
                        Due: {selectedMsg.action_deadline}
                      </span>
                    )}
                    {selectedMsg.is_bulk_sender && (
                      <span className="rounded bg-zinc-100 px-2 py-0.5 text-[11px] font-medium text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
                        Bulk
                      </span>
                    )}
                    {selectedMsg.ai_analyzed && (
                      <span className={`rounded px-2 py-0.5 text-[11px] font-medium ${
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
                    <SmBtn onClick={() => handleForwardMsg(selectedMsg)} title="Forward">
                      <Forward className="h-3.5 w-3.5" />
                    </SmBtn>
                    <SmBtn onClick={() => handlePrint(selectedMsg)} title="Print">
                      <Printer className="h-3.5 w-3.5" />
                    </SmBtn>
                    <SmBtn onClick={() => handleDownloadEml(selectedMsg.uid, selectedMsg.subject)} title="Download .eml">
                      <Download className="h-3.5 w-3.5" />
                    </SmBtn>
                    <FeedbackMenu senderEmail={extractEmail(selectedMsg.sender)} />
                  </div>
                </div>
              </div>

              {/* structured data card */}
              {selectedMsg.structured_data && (
                <StructuredDataCard data={selectedMsg.structured_data} />
              )}

              {/* AI analysis */}
              <AiAnalysisPanel message={selectedMsg} />

              {/* email body */}
              <div className="flex-1 overflow-y-auto">
                {selectedMsg.html_body && (
                  <div className="border-b border-zinc-200 dark:border-zinc-800">
                    <MessageBubble uid={selectedMsg.uid} textBody={null} htmlBody={selectedMsg.html_body} attachments={[]} isOwn={false} />
                  </div>
                )}
                {(selectedMsg.clean_text || selectedMsg.text_body || !selectedMsg.html_body) && (
                  <div className="select-text px-5 py-4">
                    <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-zinc-800 dark:text-zinc-200">
                      {selectedMsg.clean_text || selectedMsg.text_body || '(no text content)'}
                    </pre>
                  </div>
                )}
                <AttachmentPreview attachments={selectedMsg.attachments} uid={selectedMsg.uid} />
              </div>
            </>
          ) : (
            <div className="flex flex-1 items-center justify-center text-zinc-400">
              <p className="text-sm">Select a message to preview</p>
            </div>
          )}
        </div>

        {/* column 2: chat bubbles + reply editor */}
        <div className="flex w-1/2 flex-col overflow-hidden">
          {/* chat bubbles — scrollable, takes remaining space */}
          <div className="flex-1 overflow-y-auto px-4 py-3">
            {loadingThread && messages.length === 0 && (
              <div className="animate-pulse space-y-4">
                {Array.from({ length: 4 }).map((_, i) => (
                  <div key={i} className={`flex gap-2 ${i % 2 === 0 ? '' : 'flex-row-reverse'}`}>
                    <div className="h-7 w-7 shrink-0 rounded-full bg-zinc-200 dark:bg-zinc-700" />
                    <div className="h-10 w-2/3 rounded bg-zinc-200 dark:bg-zinc-700" />
                  </div>
                ))}
              </div>
            )}
            <div className="flex flex-col gap-2">
              {messages.map((msg, idx) => {
                const senderEmail = extractEmail(msg.sender)
                const isOwn = senderEmail === myEmail
                const name = extractName(msg.sender)
                const initial = avatarInitial(msg.sender)
                const color = avatarColor(msg.sender)
                const isSelected = selectedMsgIdx === idx
                const fullText = bubbleText(msg)
                const isLong = fullText.length > 200
                const snippet = isLong ? fullText.slice(0, 200) : fullText
                const isExpanded = expandedBubbles.has(idx)

                return (
                  <button
                    key={msg.id}
                    onClick={() => {
                      setSelectedMsgIdx(idx)
                      if (isLong) {
                        setExpandedBubbles((prev) => {
                          const next = new Set(prev)
                          if (next.has(idx)) next.delete(idx)
                          else next.add(idx)
                          return next
                        })
                      }
                    }}
                    className={`flex gap-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 ${isOwn ? 'flex-row-reverse' : ''}`}
                  >
                    {!isOwn && (
                      <div className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-[11px] font-medium text-white ${color}`}>
                        {initial}
                      </div>
                    )}
                    <div className={`min-w-0 max-w-[85%] ${isOwn ? 'items-end' : 'items-start'}`}>
                      <div className={`overflow-hidden rounded px-3 py-2 text-sm transition-colors ${
                        isOwn
                          ? isSelected
                            ? 'bg-blue-700 text-white'
                            : 'bg-blue-600 text-white'
                          : isSelected
                            ? 'bg-zinc-200 text-zinc-900 dark:bg-zinc-700 dark:text-zinc-100'
                            : 'bg-zinc-100 text-zinc-800 dark:bg-zinc-800 dark:text-zinc-200'
                      } ${isSelected ? 'ring-2 ring-blue-400/50' : ''}`}>
                        {!isOwn && (
                          <p className="mb-0.5 text-xs font-medium text-zinc-500 dark:text-zinc-400">{name}</p>
                        )}
                        <p className={`select-text break-words text-sm leading-snug whitespace-pre-wrap ${isExpanded ? '' : 'line-clamp-3'}`}>
                          {isExpanded ? fullText : snippet}
                        </p>
                        {isLong && (
                          <span className="mt-1 block select-none text-xs text-blue-600 dark:text-blue-400">
                            {isExpanded ? 'show less' : 'show more'}
                          </span>
                        )}
                      </div>
                      <p className={`mt-0.5 select-none text-[11px] text-zinc-400 ${isOwn ? 'text-right' : ''}`}>
                        {formatDate(msg.internal_date)}
                        {msg.attachments.length > 0 && (
                          <Paperclip className="ml-1 inline-block h-3 w-3 align-[-1px]" />
                        )}
                      </p>
                    </div>
                  </button>
                )
              })}
              <div ref={bottomRef} />
            </div>
          </div>

          {/* reply editor — large, ~40% of the column height */}
          <div className="flex h-[42%] shrink-0 flex-col border-t border-zinc-200 bg-white dark:border-zinc-800 dark:bg-zinc-950">
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
                if (m !== 'forward') setForwardSource(null)
              }}
              onSent={() => {
                setForwardSource(null)
                loadMessages(selectedId)
              }}
            />
          </div>
        </div>
      </div>
    </div>
  )
}

// box-drawing, table borders, repeated decorative lines
const NOISE_LINE = /^[\s│┼┬┴├┤┌┐└┘─━═╌╍╎╏║╔╗╚╝╠╣╦╩╬\-=_·•*#|+:>{}[\]~`]+$/

function cleanTextForBubble(raw: string): string {
  return raw
    .split('\n')
    .filter((line) => !NOISE_LINE.test(line))
    .map((line) => line.replace(/\s{2,}/g, ' ').trim())
    .filter(Boolean)
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

function bubbleText(msg: ThreadMessage): string {
  // prefer AI summary — always clean and readable
  if (msg.summary) return msg.summary
  // fall back to cleaned raw text
  const raw = msg.new_content || msg.clean_text || msg.text_body || ''
  if (!raw) return msg.subject || ''
  const cleaned = cleanTextForBubble(raw)
  return cleaned || msg.subject || ''
}

function HdrBtn({ onClick, title, className, children }: { onClick: () => void; title: string; className?: string; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className={`rounded-md p-1.5 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300 ${className ?? ''}`} title={title}>
      {children}
    </button>
  )
}

function SmBtn({ onClick, title, children }: { onClick: () => void; title: string; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className="rounded p-1 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300" title={title}>
      {children}
    </button>
  )
}

const FEEDBACK_ITEMS: { action: FeedbackAction; label: string; icon: string }[] = [
  { action: 'mark_important', label: 'Mark Important', icon: '!' },
  { action: 'mark_vip', label: 'Mark VIP', icon: '\u2605' },
  { action: 'mark_spam', label: 'Report Spam', icon: '\u26A0' },
  { action: 'block', label: 'Block Sender', icon: '\u2718' },
]

function FeedbackMenu({ senderEmail }: { senderEmail: string }) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handle = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handle)
    return () => document.removeEventListener('mousedown', handle)
  }, [open])

  const handleAction = async (action: FeedbackAction) => {
    if (action === 'block' || action === 'mark_spam') {
      const label = action === 'block' ? 'block this sender' : 'report this sender as spam'
      if (!window.confirm(`Are you sure you want to ${label}?`)) return
    }
    setOpen(false)
    if (!senderEmail) return
    try {
      const result = await recordFeedback(senderEmail, action)
      if (result.success) {
        toast.success(result.message ?? 'Feedback recorded')
      } else {
        toast.error(result.message ?? 'Failed')
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed')
    }
  }

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((p) => !p)}
        className="rounded p-1 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
        title="Sender feedback"
      >
        <MoreVertical className="h-3.5 w-3.5" />
      </button>
      {open && (
        <div className="absolute right-0 top-full z-50 mt-1 w-40 rounded border border-zinc-200 bg-white py-1 shadow-lg dark:border-zinc-700 dark:bg-zinc-900">
          <p className="truncate px-3 py-1 text-[11px] text-zinc-400">{senderEmail}</p>
          {FEEDBACK_ITEMS.map((item) => (
            <button
              key={item.action}
              onClick={() => handleAction(item.action)}
              className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                item.action === 'block' || item.action === 'mark_spam'
                  ? 'text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-900/20'
                  : 'text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800'
              }`}
            >
              <span className="w-4 text-center">{item.icon}</span>
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
