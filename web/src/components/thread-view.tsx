import type { ThreadMessage } from '@/lib/types'

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
  PanelRightClose,
  PanelRightOpen,
  Printer,
  Reply,
  Star,
  Trash2,
  X,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { AiAnalysisPanel } from '@/components/ai-analysis'
import { AttachmentPreview } from '@/components/attachment-preview'
import { BottomSheet } from '@/components/bottom-sheet'
import { Copyable } from '@/components/copy-button'
import { InviteCard } from '@/components/invite-card'
import { linkifyNodes } from '@/components/linkify-nodes'
import { MessageBubble } from '@/components/message-bubble'
import { MobileModal } from '@/components/mobile-modal'
import { ReplyBox, type ReplyMode } from '@/components/reply-box'
import { SenderAvatar } from '@/components/sender-avatar'
import { StructuredDataCard } from '@/components/structured-data-card'
import { FeedbackMenu, HdrBtn, SmBtn, ThreadTimelineItem } from '@/components/thread-view-bubble'
import { bubbleDateLabel, formatRecipients } from '@/components/thread-view-helpers'
import { useCurrentMailFilters } from '@/hooks/use-current-mail-filters'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import {
  useDeleteMutation,
  useMarkReadMutation,
  useMarkUnreadMutation,
  useStarMutation,
  useUnstarMutation,
} from '@/hooks/use-mail-mutations'
import { useThreadQuery } from '@/hooks/use-mail-queries'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { extractEmail, extractName } from '@/lib/avatar'
import { formatFullDate } from '@/lib/format'
import { highlightMentions } from '@/lib/mention'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { getToken } from '@/store/auth'
import { authAtom } from '@/store/auth'
import {
  composeReplySourceAtom,
  composingNewAtom,
  crossAccountReadAtom,
  focusedMessageUidAtom,
  mobileReplyOpenAtom,
  mobileThreadTabAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  timelineCollapsedAtom,
  visibleConversationIdsAtom,
} from '@/store/ui'

type ForwardSource = {
  body: string
  date: string
  htmlBody: null | string
  messageId: string
  sender: string
  subject: string
  uid: number
}

// Stable empty-array reference for memo'd children — without this every
// render hands MessageBubble a fresh `[]` and React.memo's shallow compare
// always says "props changed", undoing the memo wrap entirely.
const EMPTY_ATTACHMENTS: never[] = []
const EMPTY_MESSAGES: readonly ThreadMessage[] = []

export function ThreadView({ onBack }: { onBack?: () => void }) {
  const auth = useAtomValue(authAtom)
  const selectedId = useAtomValue(selectedThreadIdAtom)
  const setSelectedId = useSetAtom(selectedThreadIdAtom)
  // v2.1 2026-07-08: read `messages` straight from the RQ cache. The
  // previous local `useState<ThreadMessage[]>` mirror + bridge effect
  // repeatedly leaked stale copies of the previous thread's messages
  // into the new thread's timeline on cache-hit round-trips (5-duplicate
  // "Me" bubbles and cross-thread merges reported 2026-07-08). RQ is
  // already the canonical store — the mirror added nothing but a
  // rehydration path for bugs.
  // Subscribe only to the *selected thread's unread count* — a single
  // number — instead of the entire conversations array. Previously every
  // WebSocket-driven refetch (which produces a new array reference even
  // when no fields changed) re-rendered the entire ThreadView subtree.
  // selectAtom + Object.is equality means we only re-render when that
  // primitive actually moves. The mount-time existing-row lookup at
  // selectedId change reads imperatively via `useStore().get(...)`.
  // v2.1 phase-5b: the `selectAtom` primitive-subscription optimisation
  // moves onto RQ's `useFlatConversations` reader — same conversations
  // list, memoised flatten, `useMemo` yields a primitive that React
  // compares with Object.is (Number primitives), so unrelated array
  // changes don't re-render the ThreadView subtree.
  const currentFilters = useCurrentMailFilters()
  const { conversations: currentConversations } = useFlatConversations(currentFilters)
  const selectedUnreadCount = useMemo(() => {
    if (!selectedId) return 0
    return currentConversations.find((c) => c.thread_id === selectedId)?.unread_count ?? 0
  }, [selectedId, currentConversations])
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
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const domainsRef = useRef(selectedDomains)
  domainsRef.current = selectedDomains
  const crossAccountRead = useAtomValue(crossAccountReadAtom)
  const crossAccountReadRef = useRef(crossAccountRead)
  crossAccountReadRef.current = crossAccountRead
  const bottomRef = useRef<HTMLDivElement>(null)
  const contentScrollRef = useRef<HTMLDivElement>(null)
  const [mobileThreadTab, setMobileThreadTab] = useAtom(mobileThreadTabAtom)
  const [timelineCollapsed, setTimelineCollapsed] = useAtom(timelineCollapsedAtom)
  const [mobileReplyOpen, setMobileReplyOpen] = useAtom(mobileReplyOpenAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
  const [selectedMsgIdx, setSelectedMsgIdx] = useState<null | number>(null)
  const focusedMsgUid = useAtomValue(focusedMessageUidAtom)
  const setFocusedMsgUid = useSetAtom(focusedMessageUidAtom)
  const [isRead, setIsRead] = useState(true)
  const [isFlagged, setIsFlagged] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)
  const [replyMode, setReplyMode] = useState<ReplyMode>('reply')
  const [forwardSource, setForwardSource] = useState<ForwardSource | null>(null)
  const [showAllMessages, setShowAllMessages] = useState(false)
  // Tracks the threadId we've already auto-marked-read this entry. Set on
  // first run for a given selectedId; cleared when selectedId changes. Any
  // thread messages now live in react-query; we bridge to the legacy
  // threadMessagesAtom for downstream consumers. The bridge is structured
  // to eliminate the thread-switch flash: while a new thread is fetching
  // (selectedId already pointed at B but threadQuery.data still resolving),
  // we leave `messages` and `selectedMsgIdx` AS-IS — i.e. keep displaying
  // the previous thread — and swap atomically when the new data arrives.
  // No eager reset means no intermediate "Select a message to preview"
  // empty state, no flicker.
  const threadQuery = useThreadQuery(selectedId, selectedDomains)
  const loadingThread = threadQuery.isPending && !!selectedId
  // `messages` is the RQ query result directly — no mirror, no bridge,
  // no accumulation path. `EMPTY_MESSAGES` is a stable reference so
  // `React.memo`d children with array-typed props keep identity across
  // loading frames.
  const messages: readonly ThreadMessage[] = threadQuery.data ?? EMPTY_MESSAGES

  // On selectedId change, drop the highlighted message pointer so the
  // next paint of the timeline doesn't briefly point at the previous
  // thread's msg index. The auto-pick effect below re-seeks to the
  // last message once the new thread's data arrives.
  useEffect(() => {
    setSelectedMsgIdx(null)
  }, [selectedId])

  // Auto-pick the latest message + scroll to bottom when a thread's
  // data first resolves for the current selectedId.
  const lastResolvedRef = useRef<null | string>(null)
  useEffect(() => {
    if (!selectedId || !threadQuery.data) return
    if (lastResolvedRef.current === selectedId) return
    lastResolvedRef.current = selectedId
    setSelectedMsgIdx(threadQuery.data.length > 0 ? threadQuery.data.length - 1 : null)
    if (typeof contentScrollRef.current?.scrollTo === 'function') {
      contentScrollRef.current.scrollTo(0, 0)
    }
    if (typeof bottomRef.current?.scrollIntoView === 'function') {
      requestAnimationFrame(() => bottomRef.current?.scrollIntoView({ behavior: 'instant' }))
    }
  }, [threadQuery.data, selectedId])

  // invalidate the active thread (used after Reply / Forward send so the
  // new outbound message shows up immediately)
  const refetchThread = useCallback(() => {
    if (!selectedId) return
    queryClient.invalidateQueries({ queryKey: mailKeys.thread(selectedId) }).catch(() => {})
  }, [selectedId])

  const markReadMutation = useMarkReadMutation()
  const markUnreadMutation = useMarkUnreadMutation()
  const starMutation = useStarMutation()
  const unstarMutation = useUnstarMutation()
  const deleteMutation = useDeleteMutation()

  const handleMarkUnread = useCallback(() => {
    if (!selectedId) return
    setIsRead(false)
    markUnreadMutation.mutate(
      { threadId: selectedId },
      {
        onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed'),
        onSuccess: () => toast.success('Marked as unread'),
      }
    )
  }, [selectedId, markUnreadMutation])

  const handleMarkRead = useCallback(() => {
    if (!selectedId) return
    const doms = domainsRef.current
    const crossAll = crossAccountReadRef.current
    setIsRead(true)
    markReadMutation.mutate(
      { domains: crossAll && doms.length > 0 ? doms : undefined, threadId: selectedId },
      {
        onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed'),
        onSuccess: () => toast.success('Marked as read'),
      }
    )
  }, [selectedId, markReadMutation])

  const handleStar = useCallback(() => {
    if (!selectedId) return
    setIsFlagged(true)
    starMutation.mutate(
      { threadId: selectedId },
      { onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed') }
    )
  }, [selectedId, starMutation])

  const handleUnstar = useCallback(() => {
    if (!selectedId) return
    setIsFlagged(false)
    unstarMutation.mutate(
      { threadId: selectedId },
      { onError: (err) => toast.error(err instanceof Error ? err.message : 'Failed') }
    )
  }, [selectedId, unstarMutation])

  const handleDelete = useCallback(() => {
    if (!selectedId) return
    deleteMutation.mutate(
      { threadId: selectedId },
      {
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : 'Failed')
          setShowDeleteConfirm(false)
        },
        onSuccess: () => {
          toast.success('Deleted')
          setSelectedId(null)
          setShowDeleteConfirm(false)
          // messages come from RQ; invalidation happens via
          // `onSettled` in `useDeleteMutation`, so no local reset here.
        },
      }
    )
  }, [selectedId, deleteMutation, setSelectedId])

  const handlePrint = useCallback((msg: ThreadMessage) => {
    const w = window.open('', '_blank')
    if (!w) return
    const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    const body = msg.html_body
      ? DOMPurify.sanitize(msg.html_body)
      : `<pre style="white-space:pre-wrap;word-break:break-word;font-family:sans-serif;font-size:14px;line-height:1.6">${esc(msg.clean_text || msg.text_body || '')}</pre>`
    w.document.write(
      `<!DOCTYPE html><html><head><meta charset="utf-8"><title>${esc(msg.subject || '')}</title><style>body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;padding:2rem;max-width:800px;margin:0 auto}table{border-collapse:collapse;width:100%;margin-bottom:1.5rem}td{padding:4px 8px;font-size:14px}td:first-child{font-weight:600;white-space:nowrap;color:#555;width:80px}hr{border:none;border-top:1px solid #ddd;margin:1rem 0}img{max-width:100%}@media print{body{padding:0}}</style></head><body><table><tr><td>From</td><td>${esc(msg.sender)}</td></tr><tr><td>To</td><td>${esc(msg.recipients)}</td></tr>${msg.cc ? `<tr><td>Cc</td><td>${esc(msg.cc)}</td></tr>` : ''}<tr><td>Date</td><td>${esc(formatFullDate(msg.internal_date))}</td></tr><tr><td>Subject</td><td>${esc(msg.subject || '')}</td></tr></table><hr><div>${body}</div></body></html>`
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

  // open the full-screen composer (same UI as "new email") pre-filled as
  // a reply to this message. mirrors handleForwardMsg's shape but routes
  // through NewConversation via composeReplySourceAtom
  const handleReplyMsg = useCallback(
    (msg: ThreadMessage) => {
      if (!selectedId) return
      setComposeReplySource({
        htmlBody: msg.html_body || null,
        internalDate: msg.internal_date,
        messageId: msg.message_id,
        sender: msg.sender,
        subject: msg.subject || '',
        textBody: msg.text_body || msg.clean_text || null,
        threadId: selectedId,
        uid: msg.uid,
      })
      setComposingNew(true)
    },
    [selectedId, setComposeReplySource, setComposingNew]
  )

  useEffect(() => {
    if (!selectedId) {
      setSelectedMsgIdx(null)
      setShowDeleteConfirm(false)
      setForwardSource(null)
      return
    }
    setForwardSource(null)
    setReplyMode('reply')
    setShowAllMessages(false)
    setMobileThreadTab('content')
    setMobileReplyOpen(false)
    // v2.1 phase-5b: imperative lookup used to read
    // `store.get(conversationsAtom).find(...)`. Now walks the RQ cache
    // directly — one flatten over the current `conversationKeys.infinites`
    // entries. Latest patch (mark-read etc.) is reflected without needing
    // the atom-sync effect in chat.tsx to run first.
    const existing = currentConversations.find((c) => c.thread_id === selectedId)
    setIsRead(!existing || existing.unread_count === 0)
    setIsFlagged(existing?.flagged ?? false)
    // thread fetch is owned by useThreadQuery; nothing imperative to do here
  }, [selectedId, currentConversations, setMobileThreadTab, setMobileReplyOpen])

  // auto mark-as-read whenever the currently-displayed thread is unread.
  // covers: first open, list-filter switch where selection happens to stay
  // on the same thread, and new-message arrival on the open thread.
  // suppressed for a given selection after the user explicitly marks unread.
  // selectedUnreadCount is derived above via selectAtom — primitive,
  // re-renders only when the count itself changes

  useEffect(() => {
    if (!selectedId) return
    // Thread is already read — nothing to do.
    if (selectedUnreadCount === 0) {
      return
    }
    // Mutation in flight — the ONLY re-entry guard we need. The
    // wrapper flips pending true→false several times per successful
    // mutation cycle (onMutate → onSuccess → onSettled), and this
    // effect's deps include the mutation object, so without this
    // gate we'd re-issue the POST on every micro-transition. When the
    // mutation actually completes, the optimistic patch already set
    // unread_count = 0, so the top guard returns before we get here.
    // If the mutation errors (and we DON'T roll back — see
    // useMarkReadMutation), the patch stays, so no retry loop either.
    if (markReadMutation.isPending) return

    const doms = domainsRef.current
    const crossAll = crossAccountReadRef.current
    setIsRead(true)
    markReadMutation.mutate({
      domains: crossAll && doms.length > 0 ? doms : undefined,
      threadId: selectedId,
    })
  }, [selectedId, selectedUnreadCount, markReadMutation])

  // Smooth-scroll to the bottom of the conversation timeline only when an
  // actually-new message arrives (last message's uid changed). Previously
  // depended on the `messages` array reference, which flipped on every WS
  // refetch — even when the data was unchanged — and caused a smooth scroll
  // ~every minute the tab was open. Now: stable across refetches that don't
  // introduce a new tail message.
  const lastMessageUid = messages[messages.length - 1]?.uid
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [lastMessageUid])

  // hooks for the timeline render. Must live above the early-return below —
  // moving them after the `if (!selectedId) return …` makes the hook call
  // order vary between renders and trips classic-errors → "React hooks
  // after early return". Both work for `!selectedId`: handleSelectMsg
  // captures only the setter, timelineItems short-circuits on `messages`
  // being empty.
  const myEmailForTimeline = auth?.address ?? ''
  const VISIBLE_RECENT = 3
  const hasCollapsedTimeline = messages.length > 5 && !showAllMessages
  const handleSelectMsg = useCallback((idx: number) => setSelectedMsgIdx(idx), [])
  const timelineItems = useMemo(() => {
    const visible = hasCollapsedTimeline ? messages.slice(-VISIBLE_RECENT) : messages
    // when collapsed we slice off the tail; the global index of the first
    // visible message is offset by however many we dropped from the front.
    const offset = messages.length - visible.length
    const firstSubject = messages[0]?.subject
    let prevDateGroup = ''
    return visible.map((msg, visIdx) => {
      const idx = offset + visIdx
      const senderEmail = extractEmail(msg.sender)
      const isOwn = senderEmail === myEmailForTimeline
      const msgDateGroup = new Date(msg.internal_date * 1000).toDateString()
      const showDivider = msgDateGroup !== prevDateGroup
      prevDateGroup = msgDateGroup
      const showSubject = idx === 0 || msg.subject !== firstSubject
      return {
        dateLabel: bubbleDateLabel(msg.internal_date),
        displayName: extractName(msg.sender),
        idx,
        isOwn,
        msg,
        showDivider,
        showSubject,
        subjectText: (msg.subject || '').trim(),
      }
    })
  }, [messages, myEmailForTimeline, hasCollapsedTimeline])

  // focus a specific message when arriving from the Sent view (or a
  // shared ?msg= URL): expand the timeline if it's collapsed away, mark
  // it selected, and scroll it into view. consume the atom so it fires
  // once per navigation.
  useEffect(() => {
    if (focusedMsgUid === null || messages.length === 0) return
    const idx = messages.findIndex((m) => m.uid === focusedMsgUid)
    if (idx === -1) return
    if (hasCollapsedTimeline && idx < messages.length - VISIBLE_RECENT) {
      setShowAllMessages(true)
    }
    setSelectedMsgIdx(idx)
    const el = document.getElementById(`msg-${focusedMsgUid}`)
    if (el) {
      window.setTimeout(() => el.scrollIntoView({ behavior: 'smooth', block: 'center' }), 120)
    }
    setFocusedMsgUid(null)
  }, [focusedMsgUid, messages, hasCollapsedTimeline, setFocusedMsgUid])

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
            {/* desktop: collapse / expand conversation timeline pane */}
            <HdrBtn
              className="hidden md:inline-flex"
              onClick={() => setTimelineCollapsed((v) => !v)}
              title={timelineCollapsed ? 'Show conversation' : 'Hide conversation'}
            >
              {timelineCollapsed ? (
                <PanelRightOpen className="h-4 w-4" />
              ) : (
                <PanelRightClose className="h-4 w-4" />
              )}
            </HdrBtn>
            <HdrBtn onClick={() => setSelectedId(null)} title="Close">
              <X className="h-4 w-4" />
            </HdrBtn>
          </div>
        </div>

        {/* email body area */}
        <div className="relative flex min-h-0 flex-1 overflow-hidden">
          {loadingThread && (
            <div className="bg-bg/80 absolute inset-0 z-10 flex items-center justify-center">
              <div className="border-border border-t-accent h-5 w-5 animate-spin rounded-full border-2" />
            </div>
          )}
          {/* data-selectable: the gds base reset sets user-select:none on
              every element, and .select-text only rescues the element it's
              on — nested spans/divs stay unselectable (Chromium). gds's
              [data-selectable] * rule opts the whole reading pane back in. */}
          <div className="min-w-0 flex-1 overflow-y-auto" data-selectable ref={contentScrollRef}>
            {selectedMsg ? (
              <>
                {/* Email header (sender info). Each of the four info rows
                    has a locked height with vertical-centered content so
                    the block's total height is constant regardless of which
                    optional badges are present — switching between messages
                    no longer shifts the body downward.
                    Tags below use inline-flex h-4 leading-none so their
                    padding can't add vertical space beyond the row's box. */}
                <div className="border-border shrink-0 border-b px-4 py-2">
                  <div className="flex items-start gap-2.5">
                    <SenderAvatar className="mt-0.5" sender={selectedMsg.sender} size={28} />
                    <div className="min-w-0 flex-1 space-y-0.5">
                      <div className="flex h-5 items-center justify-between gap-2">
                        <p
                          className={`flex h-5 items-center text-sm font-medium select-text ${extractEmail(selectedMsg.sender) === myEmail ? 'text-accent' : 'text-fg'}`}
                        >
                          <span className="truncate">
                            {extractEmail(selectedMsg.sender) === myEmail
                              ? 'Me'
                              : extractName(selectedMsg.sender)}
                          </span>
                          {selectedMsg.bimi_logo_url && (
                            <img
                              alt="Verified brand"
                              className="ml-1 inline-block h-4 w-4 shrink-0"
                              height={16}
                              loading="lazy"
                              src={selectedMsg.bimi_logo_url}
                              title="BIMI verified brand"
                              width={16}
                            />
                          )}
                        </p>
                        <div className="flex h-5 shrink-0 items-center gap-0.5">
                          <SmBtn onClick={() => handleReplyMsg(selectedMsg)} title="Reply">
                            <Reply className="h-3.5 w-3.5" />
                          </SmBtn>
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
                      <p className="text-fg-muted flex h-4 items-center text-xs select-text">
                        <Copyable value={extractEmail(selectedMsg.sender)}>
                          <span className="truncate">{extractEmail(selectedMsg.sender)}</span>
                        </Copyable>
                      </p>
                      <p className="text-fg-muted flex h-4 items-center text-xs select-text">
                        <span className="truncate">
                          to {formatRecipients(selectedMsg.recipients)}
                        </span>
                      </p>
                      {selectedMsg.cc && (
                        <p className="text-fg-muted flex h-4 items-center text-xs select-text">
                          <span className="truncate">cc {formatRecipients(selectedMsg.cc)}</span>
                        </p>
                      )}
                      <div className="flex h-5 items-center gap-1.5">
                        <span className="text-fg-muted text-xs leading-none">
                          {formatFullDate(selectedMsg.internal_date)}
                        </span>
                        {selectedMsg.action_deadline && (
                          <span className="bg-warning/10 text-warning text-mini inline-flex h-4 items-center rounded px-1.5 leading-none font-medium">
                            Due: {selectedMsg.action_deadline}
                          </span>
                        )}
                        {selectedMsg.risk_score >= 40 && (
                          <span
                            className={`text-mini inline-flex h-4 items-center rounded px-1.5 leading-none font-medium ${
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

                {/* invite card (full RSVP UI) — timeline shows a compact one-liner */}
                {selectedMsg.invite_method && (
                  <div className="px-4">
                    <InviteCard messageUid={selectedMsg.uid} />
                  </div>
                )}

                {/* email body */}
                {selectedMsg.html_body && (
                  <div className="border-border border-b">
                    <MessageBubble
                      attachments={EMPTY_ATTACHMENTS}
                      htmlBody={selectedMsg.html_body}
                      isOwn={false}
                      textBody={null}
                      uid={selectedMsg.uid}
                    />
                  </div>
                )}
                {!selectedMsg.html_body && (
                  <div className="px-4 py-3 select-text">
                    <div className="text-fg text-mid font-sans leading-relaxed break-words whitespace-pre-wrap">
                      {linkifyNodes(
                        highlightMentions(
                          selectedMsg.clean_text || selectedMsg.text_body || '(no text content)',
                          myEmail,
                          auth?.display_name
                        ),
                        'text-accent no-underline hover:underline'
                      )}
                    </div>
                  </div>
                )}
                <AttachmentPreview attachments={selectedMsg.attachments} uid={selectedMsg.uid} />
              </>
            ) : loadingThread ? null : (
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

      {/* handle panel (conversation timeline + reply) — hidden on mobile content
          tab, and collapsible on desktop via the panel toggle in the header. */}
      <MPane
        className={`${mobileThreadTab === 'content' ? 'hidden' : ''} ${
          timelineCollapsed ? 'md:hidden' : 'md:flex'
        }`}
      >
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
              {hasCollapsedTimeline && (
                <button
                  className="text-accent hover:text-accent-hover mx-auto mb-2 block text-xs font-medium"
                  onClick={() => setShowAllMessages(true)}
                >
                  Show {messages.length - VISIBLE_RECENT} earlier messages
                </button>
              )}
              {timelineItems.map((item) => (
                <div id={`msg-${item.msg.uid}`} key={item.msg.uid}>
                  <ThreadTimelineItem
                    dateLabel={item.dateLabel}
                    displayName={item.displayName}
                    idx={item.idx}
                    isOwn={item.isOwn}
                    isSelected={selectedMsgIdx === item.idx}
                    msg={item.msg}
                    myEmail={myEmail}
                    myName={auth?.display_name}
                    onSelect={handleSelectMsg}
                    showDivider={item.showDivider}
                    showSubject={item.showSubject}
                    subjectText={item.subjectText}
                  />
                </div>
              ))}
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
                refetchThread()
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
                  refetchThread()
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
