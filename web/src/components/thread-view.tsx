import type { ConversationSummary, ThreadMessage } from '@/lib/types'

import { toast } from '@goliapkg/gds'
import DOMPurify from 'dompurify'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Download,
  Forward,
  Mail,
  MailOpen,
  MessageSquare,
  MoreVertical,
  Paperclip,
  Printer,
  Reply,
  Star,
  Trash2,
  X,
} from 'lucide-react'
import { Fragment, useCallback, useEffect, useRef, useState } from 'react'

import { AiAnalysisPanel } from '@/components/ai-analysis'
import { AttachmentPreview } from '@/components/attachment-preview'
import { BottomSheet } from '@/components/bottom-sheet'
import { Copyable } from '@/components/copy-button'
import { MessageBubble } from '@/components/message-bubble'
import { MobileModal } from '@/components/mobile-modal'
import { ReplyBox, type ReplyMode } from '@/components/reply-box'
import { SenderAvatar } from '@/components/sender-avatar'
import { StructuredDataCard } from '@/components/structured-data-card'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { deleteJson, type FeedbackAction, fetchJson, postJson, recordFeedback } from '@/lib/api'
import { extractEmail, extractName } from '@/lib/avatar'
import { dateGroupLabel, formatDate, formatFullDate } from '@/lib/format'
import { highlightMentions } from '@/lib/mention'
import { getToken } from '@/store/auth'
import { authAtom } from '@/store/auth'
import {
  categoryFilterAtom,
  conversationsAtom,
  crossAccountReadAtom,
  folderAtom,
  mobileReplyOpenAtom,
  mobileThreadTabAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  threadMessagesAtom,
  visibleConversationIdsAtom,
} from '@/store/chat'

type ForwardSource = {
  body: string
  date: string
  htmlBody: null | string
  messageId: string
  sender: string
  subject: string
  uid: number
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
  const visibleIds = useAtomValue(visibleConversationIdsAtom)
  const currentIdx = selectedId ? visibleIds.indexOf(selectedId) : -1
  const hasPrev = currentIdx > 0
  const hasNext = currentIdx >= 0 && currentIdx < visibleIds.length - 1
  const goToPrev = useCallback(() => {
    if (hasPrev) setSelectedId(visibleIds[currentIdx - 1])
  }, [hasPrev, visibleIds, currentIdx, setSelectedId])
  const goToNext = useCallback(() => {
    if (hasNext) setSelectedId(visibleIds[currentIdx + 1])
  }, [hasNext, visibleIds, currentIdx, setSelectedId])
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const categoryRef = useRef(categoryFilter)
  categoryRef.current = categoryFilter
  const searchQuery = useAtomValue(searchQueryAtom)
  const searchRef = useRef(searchQuery)
  searchRef.current = searchQuery
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const domainsRef = useRef(selectedDomains)
  domainsRef.current = selectedDomains
  const folder = useAtomValue(folderAtom)
  const folderRef = useRef(folder)
  folderRef.current = folder
  const crossAccountRead = useAtomValue(crossAccountReadAtom)
  const crossAccountReadRef = useRef(crossAccountRead)
  crossAccountReadRef.current = crossAccountRead
  const bottomRef = useRef<HTMLDivElement>(null)
  const contentScrollRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const [mobileThreadTab, setMobileThreadTab] = useAtom(mobileThreadTabAtom)
  const [mobileReplyOpen, setMobileReplyOpen] = useAtom(mobileReplyOpenAtom)
  const [selectedMsgIdx, setSelectedMsgIdx] = useState<null | number>(null)
  const [isRead, setIsRead] = useState(true)
  const [isFlagged, setIsFlagged] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)
  const [replyMode, setReplyMode] = useState<ReplyMode>('reply')
  const [forwardSource, setForwardSource] = useState<ForwardSource | null>(null)
  const [loadingThread, setLoadingThread] = useState(false)
  const [expandedBubbles, setExpandedBubbles] = useState<Set<number>>(new Set())
  const [showAllMessages, setShowAllMessages] = useState(false)

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
          controller.signal
        )
        if (controller.signal.aborted) return
        setMessages(data)
        if (data.length > 0) setSelectedMsgIdx(data.length - 1)
        contentScrollRef.current?.scrollTo(0, 0)
        // scroll timeline to latest message
        requestAnimationFrame(() => bottomRef.current?.scrollIntoView({ behavior: 'instant' }))

        const crossAll = crossAccountReadRef.current
        const readParam =
          crossAll && doms.length > 0 ? `?domains=${encodeURIComponent(doms.join(','))}` : ''
        postJson(`/conversations/${encodeURIComponent(threadId)}/read${readParam}`, {}).catch(
          () => {}
        )
        setIsRead(true)
        // update unread_count locally instead of re-fetching entire list
        setConversations((prev) =>
          prev.map((c) => (c.thread_id === threadId ? { ...c, unread_count: 0 } : c))
        )
      } catch (err) {
        if (!controller.signal.aborted) {
          toast.error(err instanceof Error ? err.message : 'Failed to load messages')
        }
      } finally {
        setLoadingThread(false)
      }
    },
    [setMessages, setConversations]
  )

  const refreshConversations = useCallback(async () => {
    const sq = searchRef.current
    const doms = domainsRef.current
    let path = sq
      ? `/conversations/search?q=${encodeURIComponent(sq)}&limit=50`
      : '/conversations?limit=50'
    if (categoryRef.current) path += `&category=${encodeURIComponent(categoryRef.current)}`
    if (doms.length > 0) path += `&domains=${encodeURIComponent(doms.join(','))}`
    const f = folderRef.current
    if (f) path += `&folder=${encodeURIComponent(f)}`
    try {
      const convos = await fetchJson<ConversationSummary[]>(path)
      setConversations(convos)
    } catch {
      /* ignore */
    }
  }, [setConversations])

  const handleMarkUnread = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/unread`, {})
      setIsRead(false)
      setConversations((prev) =>
        prev.map((c) =>
          c.thread_id === selectedId ? { ...c, unread_count: c.unread_count + 1 } : c
        )
      )
      toast.success('Marked as unread')
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed')
    }
  }, [selectedId, setConversations])

  const handleMarkRead = useCallback(async () => {
    if (!selectedId) return
    try {
      const doms = domainsRef.current
      const crossAll = crossAccountReadRef.current
      const domainsParam =
        crossAll && doms.length > 0 ? `?domains=${encodeURIComponent(doms.join(','))}` : ''
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/read${domainsParam}`, {})
      setIsRead(true)
      setConversations((prev) =>
        prev.map((c) => (c.thread_id === selectedId ? { ...c, unread_count: 0 } : c))
      )
      toast.success('Marked as read')
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed')
    }
  }, [selectedId, setConversations])

  const handleStar = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/star`, {})
      setIsFlagged(true)
      setConversations((prev) =>
        prev.map((c) => (c.thread_id === selectedId ? { ...c, flagged: true } : c))
      )
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed')
    }
  }, [selectedId, setConversations])

  const handleUnstar = useCallback(async () => {
    if (!selectedId) return
    try {
      await postJson(`/conversations/${encodeURIComponent(selectedId)}/unstar`, {})
      setIsFlagged(false)
      setConversations((prev) =>
        prev.map((c) => (c.thread_id === selectedId ? { ...c, flagged: false } : c))
      )
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed')
    }
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
    w.document.write(
      `<!DOCTYPE html><html><head><meta charset="utf-8"><title>${esc(msg.subject || '')}</title><style>body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;padding:2rem;max-width:800px;margin:0 auto}table{border-collapse:collapse;width:100%;margin-bottom:1.5rem}td{padding:4px 8px;font-size:14px}td:first-child{font-weight:600;white-space:nowrap;color:#555;width:80px}hr{border:none;border-top:1px solid #ddd;margin:1rem 0}img{max-width:100%}@media print{body{padding:0}}</style></head><body><table><tr><td>From</td><td>${esc(msg.sender)}</td></tr><tr><td>To</td><td>${esc(msg.recipients)}</td></tr><tr><td>Date</td><td>${esc(formatFullDate(msg.internal_date))}</td></tr><tr><td>Subject</td><td>${esc(msg.subject || '')}</td></tr></table><hr><div>${body}</div></body></html>`
    )
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
        toast.error('Download failed')
        return
      }
      const blob = await res.blob()
      const safeName = subject.replace(/[^a-zA-Z0-9\u4e00-\u9fff\u3040-\u30ff _-]/g, '_').trim()
      const url = URL.createObjectURL(blob)
      try {
        const a = document.createElement('a')
        a.href = url
        a.download = safeName ? `${safeName}.eml` : `message-${uid}.eml`
        document.body.appendChild(a)
        a.click()
        document.body.removeChild(a)
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 1000)
      }
    } catch {
      toast.error('Download failed')
    }
  }, [])

  const handleForwardMsg = useCallback((msg: ThreadMessage) => {
    setForwardSource({
      body: msg.text_body || msg.clean_text || '',
      date: formatFullDate(msg.internal_date),
      htmlBody: msg.html_body || null,
      messageId: msg.message_id,
      sender: msg.sender,
      subject: msg.subject || '',
      uid: msg.uid,
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
    setShowAllMessages(false)
    setMobileThreadTab('content')
    setMobileReplyOpen(false)
    const existing = conversationsRef.current.find((c) => c.thread_id === selectedId)
    setIsRead(!existing || existing.unread_count === 0)
    setIsFlagged(existing?.flagged ?? false)
    loadMessages(selectedId)
    return () => {
      abortRef.current?.abort()
    }
  }, [selectedId, loadMessages, setMessages, setMobileThreadTab, setMobileReplyOpen])

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  // empty state
  if (!selectedId) {
    return (
      <MPane center>
        <div className="text-fg-muted text-center">
          <Mail className="mx-auto mb-3 h-10 w-10" strokeWidth={1.5} />
          <p className="text-sm font-medium">No conversation selected</p>
          <p className="mt-1 text-xs">Choose an email from the list to read it here</p>
        </div>
      </MPane>
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
        const recipientEmails = lastMsg.recipients
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean)
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
  const fwdMsg = forwardSource ? null : (selectedMsg ?? lastMsg)
  const fwdOriginalBody =
    forwardSource?.body ?? fwdMsg?.text_body ?? fwdMsg?.clean_text ?? lastMsgBody
  const fwdOriginalHtml = forwardSource?.htmlBody ?? fwdMsg?.html_body ?? null
  const fwdUid = forwardSource?.uid ?? fwdMsg?.uid ?? null
  const fwdMessageId = forwardSource?.messageId ?? fwdMsg?.message_id ?? null
  const fwdLastMessageId = forwardSource?.messageId ?? lastMsg?.message_id ?? ''

  return (
    <MPaneGroup>
      {/* content panel — full width on mobile, flex-[2] on desktop */}
      <MPane className={`flex-[2] ${mobileThreadTab === 'conversation' ? 'hidden md:flex' : ''}`}>
        {/* header bar at top of content panel */}
        <div className="border-border flex shrink-0 items-center gap-2 border-b px-3 py-1.5 select-none">
          {onBack && (
            <button
              className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary shrink-0 rounded-md p-1 md:hidden"
              onClick={onBack}
              title="Back"
            >
              <ArrowLeft className="h-4 w-4" />
            </button>
          )}
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <h2 className="text-fg truncate text-sm font-semibold select-text">
              {subject || '(no subject)'}
            </h2>
            {messages.length > 1 && (
              <span className="text-fg-muted shrink-0 text-xs">
                {selectedMsgIdx != null ? `${selectedMsgIdx + 1}/` : ''}
                {messages.length}
              </span>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <HdrBtn
              className={hasPrev ? '' : 'pointer-events-none opacity-30'}
              onClick={goToPrev}
              title="Previous conversation"
            >
              <ChevronUp className="h-4 w-4" />
            </HdrBtn>
            <HdrBtn
              className={hasNext ? '' : 'pointer-events-none opacity-30'}
              onClick={goToNext}
              title="Next conversation"
            >
              <ChevronDown className="h-4 w-4" />
            </HdrBtn>
            <HdrBtn
              onClick={isRead ? handleMarkUnread : handleMarkRead}
              title={isRead ? 'Mark unread' : 'Mark read'}
            >
              {isRead ? <Mail className="h-4 w-4" /> : <MailOpen className="h-4 w-4" />}
            </HdrBtn>
            <HdrBtn
              className={isFlagged ? 'text-warning hover:text-warning' : undefined}
              onClick={isFlagged ? handleUnstar : handleStar}
              title={isFlagged ? 'Unstar' : 'Star'}
            >
              <Star className="h-4 w-4" fill={isFlagged ? 'currentColor' : 'none'} />
            </HdrBtn>
            <HdrBtn
              className="hover:text-danger"
              onClick={() => setShowDeleteConfirm(true)}
              title="Delete"
            >
              <Trash2 className="h-4 w-4" />
            </HdrBtn>
            {/* mobile: toggle to conversation timeline */}
            {messages.length > 1 && (
              <HdrBtn
                className="md:hidden"
                onClick={() => setMobileThreadTab('conversation')}
                title="Thread"
              >
                <MessageSquare className="h-4 w-4" />
              </HdrBtn>
            )}
            <HdrBtn onClick={() => setSelectedId(null)} title="Close">
              <X className="h-4 w-4" />
            </HdrBtn>
          </div>
        </div>

        {/* email body area */}
        <div className="relative flex min-h-0 flex-1 overflow-hidden">
          {loadingThread && messages.length > 0 && (
            <div className="bg-bg/80 absolute inset-0 z-10 flex items-center justify-center">
              <div className="border-border border-t-accent h-5 w-5 animate-spin rounded-full border-2" />
            </div>
          )}
          <div className="min-w-0 flex-1 overflow-y-auto" ref={contentScrollRef}>
            {selectedMsg ? (
              <>
                {/* email header (sender info) */}
                <div className="border-border shrink-0 border-b px-4 py-2">
                  <div className="flex items-start gap-2.5">
                    <SenderAvatar className="mt-0.5" sender={selectedMsg.sender} size={28} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-2">
                        <p
                          className={`text-sm font-medium select-text ${extractEmail(selectedMsg.sender) === myEmail ? 'text-accent' : 'text-fg'}`}
                        >
                          {extractEmail(selectedMsg.sender) === myEmail
                            ? 'Me'
                            : extractName(selectedMsg.sender)}
                          {selectedMsg.bimi_logo_url && (
                            <img
                              alt="Verified brand"
                              className="ml-1 inline-block h-4 w-4 shrink-0"
                              src={selectedMsg.bimi_logo_url}
                              title="BIMI verified brand"
                            />
                          )}
                        </p>
                        <div className="flex shrink-0 items-center gap-0.5">
                          <SmBtn onClick={() => handleForwardMsg(selectedMsg)} title="Forward">
                            <Forward className="h-3.5 w-3.5" />
                          </SmBtn>
                          <SmBtn onClick={() => handlePrint(selectedMsg)} title="Print">
                            <Printer className="h-3.5 w-3.5" />
                          </SmBtn>
                          <SmBtn
                            onClick={() => handleDownloadEml(selectedMsg.uid, selectedMsg.subject)}
                            title="Download .eml"
                          >
                            <Download className="h-3.5 w-3.5" />
                          </SmBtn>
                          <FeedbackMenu senderEmail={extractEmail(selectedMsg.sender)} />
                        </div>
                      </div>
                      <p className="text-fg-muted text-xs select-text">
                        <Copyable value={extractEmail(selectedMsg.sender)}>
                          <span>{extractEmail(selectedMsg.sender)}</span>
                        </Copyable>
                      </p>
                      <p className="text-fg-muted truncate text-xs select-text">
                        to {formatRecipients(selectedMsg.recipients)}
                      </p>
                      <div className="flex items-center gap-1.5">
                        <span className="text-fg-muted text-xs">
                          {formatFullDate(selectedMsg.internal_date)}
                        </span>
                        {selectedMsg.action_deadline && (
                          <span className="bg-warning/10 text-warning rounded px-2 py-0.5 text-xs font-medium md:text-[11px]">
                            Due: {selectedMsg.action_deadline}
                          </span>
                        )}
                        {selectedMsg.risk_score >= 40 && (
                          <span
                            className={`rounded px-2 py-0.5 text-xs font-medium md:text-[11px] ${
                              selectedMsg.risk_score >= 60
                                ? 'bg-danger/10 text-danger'
                                : 'bg-warning/10 text-warning'
                            }`}
                          >
                            {selectedMsg.risk_score >= 60 ? 'Dangerous' : 'Suspicious'}
                          </span>
                        )}
                      </div>
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
                {selectedMsg.html_body && (
                  <div className="border-border border-b">
                    <MessageBubble
                      attachments={[]}
                      htmlBody={selectedMsg.html_body}
                      isOwn={false}
                      textBody={null}
                      uid={selectedMsg.uid}
                    />
                  </div>
                )}
                {!selectedMsg.html_body && (
                  <div className="px-4 py-3 select-text">
                    <div className="text-fg font-sans text-[13px] leading-relaxed break-words whitespace-pre-wrap">
                      {highlightMentions(
                        selectedMsg.clean_text || selectedMsg.text_body || '(no text content)',
                        myEmail,
                        auth?.display_name
                      )}
                    </div>
                  </div>
                )}
                <AttachmentPreview attachments={selectedMsg.attachments} uid={selectedMsg.uid} />
              </>
            ) : (
              <div className="text-fg-muted flex h-full flex-col items-center justify-center gap-2 py-12 text-sm">
                <Mail className="h-8 w-8" strokeWidth={1.5} />
                <p>Select a message to preview</p>
              </div>
            )}
          </div>
        </div>

        {/* mobile: floating reply button */}
        <button
          className="bg-accent fixed right-4 z-30 flex h-14 w-14 items-center justify-center rounded-full text-white shadow-lg active:opacity-80 md:hidden"
          onClick={() => setMobileReplyOpen(true)}
          style={{ bottom: 'calc(60px + var(--safe-area-bottom))' }}
          title="Reply"
        >
          <Reply className="h-6 w-6" />
        </button>
      </MPane>

      {/* handle panel (conversation timeline + reply) — hidden on mobile content tab */}
      <MPane className={mobileThreadTab === 'content' ? 'hidden md:flex' : ''}>
        {/* panel header */}
        <div className="border-border flex shrink-0 items-center gap-2 border-b px-4 py-1.5 select-none">
          <button
            className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary shrink-0 rounded-md p-1 md:hidden"
            onClick={() => setMobileThreadTab('content')}
            title="Back to email"
          >
            <ArrowLeft className="h-4 w-4" />
          </button>
          <span className="text-fg-muted text-xs font-medium">
            Conversation{messages.length > 1 ? ` (${messages.length})` : ''}
          </span>
        </div>
        {/* timeline + reply box */}
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 basis-0 overflow-y-auto px-4 py-3 md:flex-[3]">
            {loadingThread && messages.length === 0 && (
              <div className="animate-pulse space-y-4">
                {Array.from({ length: 4 }).map((_, i) => (
                  <div className="border-border flex gap-3 border-b py-3" key={i}>
                    <div className="bg-border h-7 w-7 shrink-0 rounded-full" />
                    <div className="min-w-0 flex-1 space-y-2">
                      <div className="flex items-center gap-2">
                        <div className="bg-border h-3.5 w-20 rounded" />
                        <div className="bg-border h-3 w-12 rounded" />
                      </div>
                      <div className="bg-border h-10 w-full rounded" />
                    </div>
                  </div>
                ))}
              </div>
            )}
            <div className="flex flex-col gap-2">
              {(() => {
                const VISIBLE_RECENT = 3
                const hasCollapsed = messages.length > 5 && !showAllMessages
                const visibleMessages = hasCollapsed ? messages.slice(-VISIBLE_RECENT) : messages
                let prevDateGroup = ''

                return (
                  <>
                    {hasCollapsed && (
                      <button
                        className="text-accent hover:text-accent-hover mx-auto mb-2 block text-xs font-medium"
                        onClick={() => setShowAllMessages(true)}
                      >
                        Show {messages.length - VISIBLE_RECENT} earlier messages
                      </button>
                    )}
                    {visibleMessages.map((msg) => {
                      const idx = messages.indexOf(msg)
                      const name = extractName(msg.sender)
                      const senderEmail = extractEmail(msg.sender)
                      const isOwn = senderEmail === myEmail
                      const isSelected = selectedMsgIdx === idx
                      const fullText = bubbleText(msg)
                      const isLong = fullText.length > 300
                      const snippet = isLong ? smartTruncate(fullText, 300) : fullText
                      const isExpanded = expandedBubbles.has(idx)

                      const msgDateGroup = new Date(msg.internal_date * 1000).toDateString()
                      const showDivider = msgDateGroup !== prevDateGroup
                      prevDateGroup = msgDateGroup

                      return (
                        <Fragment key={msg.id}>
                          {showDivider && (
                            <BubbleDateDivider label={bubbleDateLabel(msg.internal_date)} />
                          )}
                          <div
                            className={`focus-visible:ring-accent/50 flex cursor-pointer gap-3 rounded-lg px-3 py-2.5 transition-colors focus-visible:ring-2 focus-visible:outline-none ${
                              isSelected ? 'bg-accent/10' : 'hover:bg-bg-secondary'
                            } ${isOwn ? 'ml-6' : ''}`}
                            onClick={() => setSelectedMsgIdx(idx)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter' || e.key === ' ') e.currentTarget.click()
                            }}
                            role="button"
                            tabIndex={0}
                          >
                            <SenderAvatar sender={msg.sender} size={28} />
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2">
                                <span
                                  className={`text-sm font-semibold ${isOwn ? 'text-accent' : 'text-fg'}`}
                                >
                                  {isOwn ? 'Me' : name}
                                </span>
                                <span className="text-fg-muted text-xs">
                                  {formatDate(msg.internal_date)}
                                  {msg.attachments.length > 0 && (
                                    <Paperclip className="ml-1 inline-block h-3 w-3 align-[-1px]" />
                                  )}
                                </span>
                              </div>
                              <div className="relative mt-1">
                                <div
                                  className={`text-fg text-[13px] leading-relaxed select-text ${isExpanded ? '' : 'line-clamp-5'}`}
                                >
                                  {highlightMentions(
                                    isExpanded ? fullText : snippet,
                                    myEmail,
                                    auth?.display_name
                                  )}
                                </div>
                                {isLong && !isExpanded && (
                                  <div
                                    className={`absolute right-0 bottom-0 left-0 h-6 bg-gradient-to-t ${isSelected ? 'from-accent/10' : 'from-surface'}`}
                                  />
                                )}
                              </div>
                              {isLong && (
                                <button
                                  className="text-accent mt-1.5 block text-xs font-medium select-none hover:underline"
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    setExpandedBubbles((prev) => {
                                      const next = new Set(prev)
                                      if (next.has(idx)) next.delete(idx)
                                      else next.add(idx)
                                      return next
                                    })
                                  }}
                                  type="button"
                                >
                                  {isExpanded ? 'show less' : 'show more'}
                                </button>
                              )}
                            </div>
                          </div>
                        </Fragment>
                      )
                    })}
                  </>
                )
              })()}
              <div ref={bottomRef} />
            </div>
          </div>
          <div className="border-border hidden min-h-[160px] flex-[1] basis-0 flex-col border-t md:flex">
            <ReplyBox
              forwardAttachmentsUid={fwdUid}
              forwardMessageId={fwdMessageId}
              lastMessageId={fwdLastMessageId}
              mode={replyMode}
              onModeChange={(m) => {
                setReplyMode(m)
                if (m !== 'forward') setForwardSource(null)
              }}
              onSent={() => {
                setForwardSource(null)
                loadMessages(selectedId)
              }}
              originalBody={fwdOriginalBody}
              originalDate={fwdOriginalDate}
              originalFrom={fwdOriginalFrom}
              originalHtmlBody={fwdOriginalHtml}
              replyAllRecipients={replyAllRecipients || extractEmail(messages[0]?.sender ?? '')}
              replyRecipients={replyRecipients || extractEmail(messages[0]?.sender ?? '')}
              subject={fwdSubject}
              threadId={selectedId}
            />
          </div>
        </div>
      </MPane>

      {/* mobile: full-screen reply composer */}
      {mobileReplyOpen && (
        <MobileModal className="items-end md:hidden" onClose={() => setMobileReplyOpen(false)} open>
          <div
            className="bg-surface flex h-[90dvh] w-full flex-col rounded-t-2xl"
            onClick={(e) => e.stopPropagation()}
            style={{ paddingBottom: 'var(--safe-area-bottom)' }}
          >
            {/* header */}
            <div className="border-border flex shrink-0 items-center justify-between border-b px-4 py-3">
              <button
                className="text-fg-muted hover:text-fg-secondary"
                onClick={() => setMobileReplyOpen(false)}
              >
                <X className="h-5 w-5" />
              </button>
              <span className="text-fg truncate text-sm font-medium">
                {subject || '(no subject)'}
              </span>
              <div className="w-5" />
            </div>
            {/* reply box with full height */}
            <div className="min-h-0 flex-1">
              <ReplyBox
                forwardAttachmentsUid={fwdUid}
                forwardMessageId={fwdMessageId}
                lastMessageId={fwdLastMessageId}
                mode={replyMode}
                onModeChange={(m) => {
                  setReplyMode(m)
                  if (m !== 'forward') setForwardSource(null)
                }}
                onSent={() => {
                  setForwardSource(null)
                  setMobileReplyOpen(false)
                  loadMessages(selectedId)
                }}
                originalBody={fwdOriginalBody}
                originalDate={fwdOriginalDate}
                originalFrom={fwdOriginalFrom}
                originalHtmlBody={fwdOriginalHtml}
                replyAllRecipients={replyAllRecipients || extractEmail(messages[0]?.sender ?? '')}
                replyRecipients={replyRecipients || extractEmail(messages[0]?.sender ?? '')}
                subject={fwdSubject}
                threadId={selectedId}
              />
            </div>
          </div>
        </MobileModal>
      )}

      {/* delete confirm dialog */}
      {showDeleteConfirm && (
        <BottomSheet onClose={() => setShowDeleteConfirm(false)} open>
          <h3 className="text-fg text-sm font-semibold">Delete conversation?</h3>
          <p className="text-fg-muted mt-1.5 text-sm">This will permanently delete all messages.</p>
          <div className="mt-4 flex justify-end gap-2">
            <button
              className="border-border text-fg-secondary hover:bg-bg-secondary rounded-md border px-3 py-2 text-sm transition-colors"
              onClick={() => setShowDeleteConfirm(false)}
            >
              Cancel
            </button>
            <button
              className="bg-danger rounded-md px-3 py-2 text-sm font-medium text-white transition-colors hover:opacity-90"
              onClick={handleDelete}
            >
              Delete
            </button>
          </div>
        </BottomSheet>
      )}
    </MPaneGroup>
  )
}

function BubbleDateDivider({ label }: { label: string }) {
  return (
    <div className="flex justify-center py-2 select-none">
      <span className="bg-bg-secondary text-fg-muted rounded-full px-2.5 py-0.5 text-xs font-medium md:text-[10px]">
        {label}
      </span>
    </div>
  )
}

const bubbleDateLabel = (ts: number | string) =>
  dateGroupLabel(typeof ts === 'number' ? ts : Math.floor(new Date(ts).getTime() / 1000))

// strip invisible unicode: ZWJ, ZWNJ, ZW space, BOM, soft hyphen, directional marks, etc.
const INVISIBLE_RE =
  // eslint-disable-next-line no-misleading-character-class
  /[\u200B-\u200F\u2028-\u202F\u2060-\u2064\uFEFF\u00AD\u034F\u061C\u180E]/g

// box-drawing, table borders, repeated decorative lines
const NOISE_LINE = /^[\s│┼┬┴├┤┌┐└┘─━═╌╍╎╏║╔╗╚╝╠╣╦╩╬\-=_·•*#|+:>{}[\]~`]+$/

function bubbleText(msg: ThreadMessage): string {
  // prefer AI summary — always clean and readable
  if (msg.summary) return msg.summary
  // fall back to cleaned raw text
  const raw = msg.new_content || msg.clean_text || msg.text_body || ''
  if (!raw) return msg.subject || ''
  const cleaned = cleanTextForBubble(raw)
  return cleaned || msg.subject || ''
}

function cleanTextForBubble(raw: string): string {
  const lines = raw.replace(INVISIBLE_RE, '').split('\n')

  // find signature delimiter and remove everything after it
  let sigIdx = lines.length
  for (let i = 0; i < lines.length; i++) {
    if (lines[i] === '-- ' || lines[i] === '--') {
      sigIdx = i
      break
    }
  }

  return lines
    .slice(0, sigIdx)
    .filter((line) => !NOISE_LINE.test(line))
    .filter((line) => !line.startsWith('>')) // remove quoted lines
    .map((line) => line.replace(/\s{2,}/g, ' ').trim())
    .filter(Boolean)
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

// format recipients string into a short human-readable form
function formatRecipients(recipients: string): string {
  // split by comma, extract names, deduplicate
  const parts = recipients
    .split(',')
    .map((r) => extractName(r.trim()))
    .filter(Boolean)
  if (parts.length === 0) return recipients
  if (parts.length <= 2) return parts.join(', ')
  return `${parts[0]}, ${parts[1]} +${parts.length - 2}`
}

function HdrBtn({
  children,
  className,
  onClick,
  title,
}: {
  children: React.ReactNode
  className?: string
  onClick: () => void
  title: string
}) {
  return (
    <button
      className={`text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-7 w-7 items-center justify-center rounded-md transition-colors ${className ?? ''}`}
      onClick={onClick}
      title={title}
    >
      {children}
    </button>
  )
}

// truncate at nearest sentence/paragraph boundary instead of hard character cut
function smartTruncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text
  const sub = text.slice(0, maxLen)
  // try to break at paragraph
  const lastNewline = sub.lastIndexOf('\n')
  if (lastNewline > maxLen * 0.5) return sub.slice(0, lastNewline).trimEnd()
  // try to break at sentence (。.!?！？)
  const sentenceEnd = sub.search(/[.。!！?？]\s*[^\s]*$/)
  if (sentenceEnd > maxLen * 0.4) return sub.slice(0, sentenceEnd + 1).trimEnd()
  // fall back to word boundary
  const lastSpace = sub.lastIndexOf(' ')
  if (lastSpace > maxLen * 0.5) return sub.slice(0, lastSpace).trimEnd() + '…'
  return sub.trimEnd() + '…'
}

function SmBtn({
  children,
  onClick,
  title,
}: {
  children: React.ReactNode
  onClick: () => void
  title: string
}) {
  return (
    <button
      className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-7 w-7 items-center justify-center rounded-md transition-all duration-150"
      onClick={onClick}
      title={title}
    >
      {children}
    </button>
  )
}

const FEEDBACK_ITEMS: {
  action: FeedbackAction
  icon: string
  label: string
}[] = [
  { action: 'mark_important', icon: '!', label: 'Mark Important' },
  { action: 'mark_vip', icon: '\u2605', label: 'Mark VIP' },
  { action: 'mark_spam', icon: '\u26A0', label: 'Report Spam' },
  { action: 'block', icon: '\u2718', label: 'Block Sender' },
]

function FeedbackMenu({ senderEmail }: { senderEmail: string }) {
  const [open, setOpen] = useState(false)
  const [confirming, setConfirming] = useState<FeedbackAction | null>(null)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handle = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false)
        setConfirming(null)
      }
    }
    document.addEventListener('mousedown', handle)
    return () => document.removeEventListener('mousedown', handle)
  }, [open])

  const executeAction = async (action: FeedbackAction) => {
    setOpen(false)
    setConfirming(null)
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

  const handleAction = (action: FeedbackAction) => {
    if (action === 'block' || action === 'mark_spam') {
      setConfirming(action)
    } else {
      executeAction(action)
    }
  }

  return (
    <div className="relative" ref={ref}>
      <button
        className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary rounded-md p-1 transition-colors"
        onClick={() => setOpen((p) => !p)}
        title="Sender feedback"
      >
        <MoreVertical className="h-3.5 w-3.5" />
      </button>
      {open && (
        <div className="border-border bg-surface absolute top-full right-0 z-50 mt-1 w-48 rounded-lg border py-1 shadow-lg">
          {confirming ? (
            <div className="px-3 py-2">
              <p className="text-fg-secondary text-xs">
                {confirming === 'block' ? 'Block this sender?' : 'Report as spam?'}
              </p>
              <div className="mt-2 flex gap-2">
                <button
                  className="text-fg-muted hover:bg-bg-secondary rounded px-2 py-1 text-xs"
                  onClick={() => setConfirming(null)}
                >
                  Cancel
                </button>
                <button
                  className="bg-danger rounded px-2 py-1 text-xs text-white hover:opacity-90"
                  onClick={() => executeAction(confirming)}
                >
                  Confirm
                </button>
              </div>
            </div>
          ) : (
            <>
              <p className="text-fg-muted truncate px-3 py-1 text-xs md:text-[11px]">
                {senderEmail}
              </p>
              {FEEDBACK_ITEMS.map((item) => (
                <button
                  className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                    item.action === 'block' || item.action === 'mark_spam'
                      ? 'text-danger hover:bg-danger/10'
                      : 'text-fg-secondary hover:bg-bg-secondary'
                  }`}
                  key={item.action}
                  onClick={() => handleAction(item.action)}
                >
                  <span className="w-4 text-center">{item.icon}</span>
                  {item.label}
                </button>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  )
}
