import { useAtomValue, useSetAtom } from 'jotai'
import { ArrowLeft, Download, Forward, Mail, MailOpen, MoreVertical, Paperclip, Printer, Star, Trash2, X } from 'lucide-react'
import { Fragment, useCallback, useEffect, useRef, useState } from 'react'
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
import { highlightMentions } from '@/lib/mention'
import { deleteJson, fetchJson, postJson, recordFeedback, type FeedbackAction } from '@/lib/api'
import { getToken } from '@/store/auth'
import { formatDate, formatFullDate } from '@/lib/format'
import type { ConversationSummary, ThreadMessage } from '@/lib/types'
import { categoryFilterAtom, conversationsAtom, crossAccountReadAtom, folderAtom, searchQueryAtom, selectedDomainsAtom, selectedThreadIdAtom, threadMessagesAtom } from '@/store/chat'
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
  const folder = useAtomValue(folderAtom)
  const folderRef = useRef(folder)
  folderRef.current = folder
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
        // update unread_count locally instead of re-fetching entire list
        setConversations((prev) => prev.map((c) => c.thread_id === threadId ? { ...c, unread_count: 0 } : c))
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
    const f = folderRef.current
    if (f) path += `&folder=${encodeURIComponent(f)}`
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
    setShowAllMessages(false)
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
          <div className="flex items-center border-b border-[var(--color-border-default)] px-4 py-3 md:hidden">
            <button onClick={onBack} className="flex items-center gap-1.5 text-sm text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]">
              <ArrowLeft className="h-4 w-4" />
              Back
            </button>
          </div>
        )}
        <div className="flex flex-1 items-center justify-center text-[var(--color-text-tertiary)]">
          <div className="text-center">
            <Mail className="mx-auto mb-3 h-12 w-12 text-[var(--color-text-tertiary)]" strokeWidth={1} />
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
    <div className="flex flex-1 flex-col gap-1 overflow-hidden">
      {/* header bar */}
      <div className="flex shrink-0 select-none items-center gap-2 rounded-lg bg-[var(--color-bg-raised)] px-4 py-2.5">
        {onBack && (
          <button onClick={onBack} className="shrink-0 rounded-md p-1 text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)] md:hidden" title="Back">
            <ArrowLeft className="h-5 w-5" />
          </button>
        )}
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <h2 className="select-text truncate text-sm font-semibold text-[var(--color-text-primary)]">{subject || '(no subject)'}</h2>
          <span className="shrink-0 text-xs text-[var(--color-text-tertiary)]">{messages.length}</span>
          <CategoryBadge category={messages[0]?.category} />
          <ImportanceBadge level={messages[0]?.importance_level} />
          {messages.some((m) => m.requires_action) && <ActionBadge />}
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          <HdrBtn onClick={isRead ? handleMarkUnread : handleMarkRead} title={isRead ? 'Mark unread' : 'Mark read'}>
            {isRead ? (
              <Mail className="h-4 w-4" />
            ) : (
              <MailOpen className="h-4 w-4" />
            )}
          </HdrBtn>
          <HdrBtn onClick={isFlagged ? handleUnstar : handleStar} title={isFlagged ? 'Unstar' : 'Star'} className={isFlagged ? 'text-[var(--color-status-warning)] hover:text-[var(--color-status-warning)]' : undefined}>
            <Star className="h-4 w-4" fill={isFlagged ? 'currentColor' : 'none'} />
          </HdrBtn>
          <HdrBtn onClick={() => setShowDeleteConfirm(true)} title="Delete" className="hover:text-[var(--color-status-danger)]">
            <Trash2 className="h-4 w-4" />
          </HdrBtn>
          <HdrBtn onClick={() => setSelectedId(null)} title="Close">
            <X className="h-4 w-4" />
          </HdrBtn>
        </div>
      </div>

      {/* delete dialog */}
      {showDeleteConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" role="dialog" aria-modal="true" onClick={() => setShowDeleteConfirm(false)} onKeyDown={(e) => { if (e.key === 'Escape') setShowDeleteConfirm(false) }}>
          <div className="mx-4 w-full max-w-sm rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-overlay)] p-6" style={{ boxShadow: 'var(--shadow-lg)' }} onClick={(e) => e.stopPropagation()}>
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">Delete conversation?</h3>
            <p className="mt-1.5 text-sm text-[var(--color-text-tertiary)]">This will permanently delete all messages.</p>
            <div className="mt-4 flex justify-end gap-2">
              <button onClick={() => setShowDeleteConfirm(false)} className="rounded-md border border-[var(--color-border-default)] px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]">Cancel</button>
              <button onClick={handleDelete} className="rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90">Delete</button>
            </div>
          </div>
        </div>
      )}

      {/* main content: two columns */}
      <div className="flex flex-1 gap-1 overflow-hidden">
        {/* column 1: raw email (content panel) */}
        <div className="flex w-1/2 flex-col overflow-hidden rounded-lg bg-[var(--color-bg-raised)]">
          {selectedMsg ? (
            <>
              {/* email header */}
              <div className="shrink-0 border-b border-[var(--color-border-default)] px-5 py-3">
                <div className="flex items-center gap-3">
                  <div className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-xs font-medium text-white ${avatarColor(selectedMsg.sender)}`}>
                    {avatarInitial(selectedMsg.sender)}
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="flex items-center gap-1 select-text text-sm font-medium text-[var(--color-text-primary)]">
                      {extractName(selectedMsg.sender)}
                      {selectedMsg.bimi_logo_url && (
                        <img
                          src={selectedMsg.bimi_logo_url}
                          alt="Verified brand"
                          className="inline-block h-4 w-4 shrink-0"
                          title="BIMI verified brand"
                        />
                      )}
                      {' '}
                      <Copyable value={extractEmail(selectedMsg.sender)}>
                        <span className="max-w-[200px] truncate font-normal text-[var(--color-text-tertiary)]">&lt;{extractEmail(selectedMsg.sender)}&gt;</span>
                      </Copyable>
                    </p>
                    <p className="select-text truncate text-xs text-[var(--color-text-tertiary)]">
                      to {formatRecipients(selectedMsg.recipients)} · {formatFullDate(selectedMsg.internal_date)}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-0.5">
                    <IntentBadge intent={selectedMsg.sender_intent} />
                    {selectedMsg.action_deadline && (
                      <span className="bg-[var(--color-status-warning-subtle)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-status-warning)]">
                        Due: {selectedMsg.action_deadline}
                      </span>
                    )}
                    {selectedMsg.is_bulk_sender && (
                      <span className="bg-[var(--color-bg-raised)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-tertiary)]">
                        Bulk
                      </span>
                    )}
                    {selectedMsg.ai_analyzed && (
                      <span className={`px-2 py-0.5 text-[11px] font-medium ${
                        selectedMsg.risk_score >= 60
                          ? 'bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]'
                          : selectedMsg.risk_score >= 40
                            ? 'bg-[var(--color-status-warning-subtle)] text-[var(--color-status-warning)]'
                            : selectedMsg.risk_score >= 15
                              ? 'bg-[var(--color-status-info-subtle)] text-[var(--color-status-info)]'
                              : 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]'
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
                  <div className="border-b border-[var(--color-border-default)]">
                    <MessageBubble uid={selectedMsg.uid} textBody={null} htmlBody={selectedMsg.html_body} attachments={[]} isOwn={false} />
                  </div>
                )}
                {(selectedMsg.clean_text || selectedMsg.text_body || !selectedMsg.html_body) && (
                  <div className="select-text px-5 py-4">
                    <div className="whitespace-pre-wrap break-words font-sans text-[13px] leading-relaxed text-[var(--color-text-primary)]">
                      {highlightMentions(selectedMsg.clean_text || selectedMsg.text_body || '(no text content)', myEmail, auth?.display_name)}
                    </div>
                  </div>
                )}
                <AttachmentPreview attachments={selectedMsg.attachments} uid={selectedMsg.uid} />
              </div>
            </>
          ) : (
            <div className="flex flex-1 items-center justify-center text-[var(--color-text-tertiary)]">
              <p className="text-sm">Select a message to preview</p>
            </div>
          )}
        </div>

        {/* column 2: chat bubbles + reply editor */}
        <div className="flex w-1/2 flex-col overflow-hidden rounded-lg bg-[var(--color-bg-raised)]">
          {/* panel header */}
          <div className="flex shrink-0 select-none items-center border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-4 py-1.5">
            <span className="text-xs font-medium text-[var(--color-text-tertiary)]">
              Conversation
              {messages.length > 0 && <span className="ml-1.5 text-[var(--color-text-tertiary)]">({messages.length})</span>}
            </span>
          </div>
          {/* chat bubbles — scrollable, takes remaining space */}
          <div className="flex-1 overflow-y-auto px-4 py-3">
            {loadingThread && messages.length === 0 && (
              <div className="animate-pulse space-y-4">
                {Array.from({ length: 4 }).map((_, i) => (
                  <div key={i} className="flex gap-3 border-b border-[var(--color-border-default)] py-3">
                    <div className="h-7 w-7 shrink-0 rounded-full bg-[var(--color-border-default)]" />
                    <div className="min-w-0 flex-1 space-y-2">
                      <div className="flex items-center gap-2">
                        <div className="h-3.5 w-20 rounded bg-[var(--color-border-default)]" />
                        <div className="h-3 w-12 rounded bg-[var(--color-border-default)]" />
                      </div>
                      <div className="h-10 w-full rounded bg-[var(--color-border-default)]" />
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
                        onClick={() => setShowAllMessages(true)}
                        className="mx-auto mb-2 block text-xs font-medium text-[var(--color-brand-primary)] hover:text-[var(--color-brand-primary-hover)]"
                      >
                        Show {messages.length - VISIBLE_RECENT} earlier messages
                      </button>
                    )}
                    {visibleMessages.map((msg) => {
                      const idx = messages.indexOf(msg)
                      const name = extractName(msg.sender)
                      const initial = avatarInitial(msg.sender)
                      const color = avatarColor(msg.sender)
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
                          {showDivider && <BubbleDateDivider label={bubbleDateLabel(msg.internal_date)} />}
                          <div
                            role="button"
                            tabIndex={0}
                            onClick={() => setSelectedMsgIdx(idx)}
                            onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') e.currentTarget.click() }}
                            className={`flex cursor-pointer gap-3 border-b border-[var(--color-border-default)] py-3 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)] ${
                              isSelected ? 'bg-[var(--color-bg-selected)]' : ''
                            }`}
                          >
                            <div className="shrink-0">
                              <div className={`flex h-7 w-7 items-center justify-center rounded-full text-[11px] font-medium text-white ${color}`}>
                                {initial}
                              </div>
                            </div>
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2">
                                <span className="text-sm font-semibold text-[var(--color-text-primary)]">{name}</span>
                                <span className="text-xs text-[var(--color-text-tertiary)]">
                                  {formatDate(msg.internal_date)}
                                  {msg.attachments.length > 0 && (
                                    <Paperclip className="ml-1 inline-block h-3 w-3 align-[-1px]" />
                                  )}
                                </span>
                              </div>
                              <div className={`mt-1 select-text text-sm leading-relaxed text-[var(--color-text-primary)] ${isExpanded ? '' : 'line-clamp-5'}`}>
                                {highlightMentions(isExpanded ? fullText : snippet, myEmail, auth?.display_name)}
                              </div>
                              {isLong && (
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    setExpandedBubbles((prev) => {
                                      const next = new Set(prev)
                                      if (next.has(idx)) next.delete(idx)
                                      else next.add(idx)
                                      return next
                                    })
                                  }}
                                  className="mt-1.5 block select-none text-xs font-medium text-[var(--color-brand-primary)] hover:underline"
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

          {/* reply editor — large, ~40% of the column height */}
          <div className="flex h-[42%] shrink-0 flex-col border-t-2 border-[var(--color-border-strong)] bg-[var(--color-bg-sunken)]">
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

function BubbleDateDivider({ label }: { label: string }) {
  return (
    <div className="flex select-none justify-center py-2">
      <span className="rounded-full bg-[var(--color-bg-sunken)] px-2.5 py-0.5 text-[10px] font-medium text-[var(--color-text-tertiary)]">{label}</span>
    </div>
  )
}

function bubbleDateLabel(dateStr: string | number): string {
  const d = new Date(typeof dateStr === 'number' ? dateStr * 1000 : dateStr)
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const msgDate = new Date(d.getFullYear(), d.getMonth(), d.getDate())
  const diffDays = Math.floor((today.getTime() - msgDate.getTime()) / 86400000)
  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays < 7) return d.toLocaleDateString(undefined, { weekday: 'long' })
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: now.getFullYear() !== d.getFullYear() ? 'numeric' : undefined })
}

// strip invisible unicode: ZWJ, ZWNJ, ZW space, BOM, soft hyphen, directional marks, etc.
const INVISIBLE_RE = /[\u200B-\u200F\u2028-\u202F\u2060-\u2064\uFEFF\u00AD\u034F\u061C\u180E]/g

// box-drawing, table borders, repeated decorative lines
const NOISE_LINE = /^[\s│┼┬┴├┤┌┐└┘─━═╌╍╎╏║╔╗╚╝╠╣╦╩╬\-=_·•*#|+:>{}[\]~`]+$/

function cleanTextForBubble(raw: string): string {
  const lines = raw
    .replace(INVISIBLE_RE, '')
    .split('\n')

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

function bubbleText(msg: ThreadMessage): string {
  // prefer AI summary — always clean and readable
  if (msg.summary) return msg.summary
  // fall back to cleaned raw text
  const raw = msg.new_content || msg.clean_text || msg.text_body || ''
  if (!raw) return msg.subject || ''
  const cleaned = cleanTextForBubble(raw)
  return cleaned || msg.subject || ''
}

// format recipients string into a short human-readable form
function formatRecipients(recipients: string): string {
  // split by comma, extract names, deduplicate
  const parts = recipients.split(',').map((r) => extractName(r.trim())).filter(Boolean)
  if (parts.length === 0) return recipients
  if (parts.length <= 2) return parts.join(', ')
  return `${parts[0]}, ${parts[1]} +${parts.length - 2}`
}

function HdrBtn({ onClick, title, className, children }: { onClick: () => void; title: string; className?: string; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className={`flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-active)] hover:text-[var(--color-text-secondary)] ${className ?? ''}`} title={title}>
      {children}
    </button>
  )
}

function SmBtn({ onClick, title, children }: { onClick: () => void; title: string; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-all duration-150 hover:bg-[var(--color-active)] hover:text-[var(--color-text-secondary)]" title={title}>
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
        className="rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]"
        title="Sender feedback"
      >
        <MoreVertical className="h-3.5 w-3.5" />
      </button>
      {open && (
        <div className="absolute right-0 top-full z-50 mt-1 w-40 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-overlay)] py-1" style={{ boxShadow: 'var(--shadow-lg)' }}>
          <p className="truncate px-3 py-1 text-[11px] text-[var(--color-text-tertiary)]">{senderEmail}</p>
          {FEEDBACK_ITEMS.map((item) => (
            <button
              key={item.action}
              onClick={() => handleAction(item.action)}
              className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                item.action === 'block' || item.action === 'mark_spam'
                  ? 'text-[var(--color-status-danger)] hover:bg-[var(--color-status-danger-subtle)]'
                  : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
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
