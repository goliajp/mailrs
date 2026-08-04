import type { ThreadMessage } from '@/lib/types'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { Mail } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { type ReplyMode } from '@/components/reply-box'
import { ThreadContentPane } from '@/components/thread-content-pane'
import { ThreadConversationPane, VISIBLE_RECENT } from '@/components/thread-conversation-pane'
import { type ThreadReplyContext } from '@/components/thread-reply-box'
import { ThreadViewDialogs } from '@/components/thread-view-dialogs'
import {
  useConversationRows,
  useCurrentListRows,
  useCurrentSelection,
  useSelectThreadId,
} from '@/hooks/use-current-list'
import { useThreadQuery } from '@/hooks/use-mail-queries'
import { useThreadActions } from '@/hooks/use-thread-actions'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { extractEmail } from '@/lib/avatar'
import { formatFullDate } from '@/lib/format'
import { defaultReadingTarget } from '@/lib/thread-reading'
import { authAtom } from '@/store/auth'
import {
  composeReplySourceAtom,
  composingNewAtom,
  crossAccountReadAtom,
  mobileReplyOpenAtom,
  mobileThreadTabAtom,
  selectedDomainsAtom,
  timelineCollapsedAtom,
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
const EMPTY_MESSAGES: readonly ThreadMessage[] = []

export function ThreadView({ onBack }: { onBack?: () => void }) {
  const auth = useAtomValue(authAtom)
  const selection = useCurrentSelection()
  const selectedId = selection?.threadId ?? null
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
  const { rows: currentConversations } = useConversationRows()
  const selectedUnreadCount = useMemo(() => {
    if (!selectedId) return 0
    return currentConversations.find((c) => c.thread_id === selectedId)?.unread_count ?? 0
  }, [selectedId, currentConversations])
  // Prev/next walk the same rows the list draws, because they are the
  // same value — this used to read an atom the list kept in step with an
  // effect, which is two copies of one list.
  const rows = useCurrentListRows()
  const setSelectedId = useSelectThreadId()
  const currentIdx = selectedId ? rows.findIndex((r) => r.threadId === selectedId) : -1
  const hasPrev = currentIdx > 0
  const hasNext = currentIdx >= 0 && currentIdx < rows.length - 1
  const goToPrev = useCallback(() => {
    if (hasPrev) setSelectedId(rows[currentIdx - 1]?.threadId ?? null)
  }, [hasPrev, rows, currentIdx, setSelectedId])
  const goToNext = useCallback(() => {
    if (hasNext) setSelectedId(rows[currentIdx + 1]?.threadId ?? null)
  }, [hasNext, rows, currentIdx, setSelectedId])
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
  // The message to scroll to, when the selected row named one — a Send
  // row is one message, a conversation row is a whole thread. It used to
  // be a separate atom the Send list wrote and this cleared, which meant
  // arriving at Send and having its first row picked for you focused
  // nothing: there was no click to write it.
  const focusedMsgUid = selection?.uid ?? null
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

  // Scroll to the bottom when a thread's data first resolves. Scrolling
  // is an action, so it belongs in an effect and is latched per thread.
  // *Which message is being read* is not an action and no longer lives
  // here — see `currentMsgIdx` below. It used to be written by this same
  // effect behind this same latch, and the two disagreed: leaving a
  // thread and coming back moves `selectedId` A -> null -> A, which
  // resets the pointer, while the latch still said "A already picked"
  // and skipped the re-pick. The result was a thread whose header and
  // timeline were both showing it and whose pane said "Select a message
  // to preview" — a state the app should not have been able to hold.
  const scrolledFor = useRef<null | string>(null)
  useEffect(() => {
    if (!selectedId || !threadQuery.data) return
    if (scrolledFor.current === selectedId) return
    scrolledFor.current = selectedId
    if (typeof contentScrollRef.current?.scrollTo === 'function') {
      contentScrollRef.current.scrollTo(0, 0)
    }
    if (typeof bottomRef.current?.scrollIntoView === 'function') {
      requestAnimationFrame(() => bottomRef.current?.scrollIntoView({ behavior: 'instant' }))
    }
  }, [threadQuery.data, selectedId])

  // invalidate the active thread (used after Reply / Forward send so the
  // new outbound message shows up immediately)
  const {
    handleDelete,
    handleMarkRead,
    handleMarkUnread,
    handleStar,
    handleUnstar,
    markReadMutation,
    refetchThread,
  } = useThreadActions({
    crossAccountReadRef,
    domainsRef,
    selectedId,
    setIsFlagged,
    setIsRead,
    setSelectedId,
    setShowDeleteConfirm,
  })
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
  const hasCollapsedTimeline = messages.length > 5 && !showAllMessages
  const handleSelectMsg = useCallback((idx: number) => setSelectedMsgIdx(idx), [])

  // Focus a specific message when the selected row names one: expand the
  // timeline if it is collapsed away, mark it selected, and scroll it
  // into view.
  //
  // Scrolling is an action, so it is guarded on the pair it was last run
  // for rather than by clearing the value that asked for it — that value
  // is now derived, and clearing a derivation only makes it come back.
  const scrolledToRef = useRef<null | string>(null)
  useEffect(() => {
    if (focusedMsgUid === null || selectedId === null || messages.length === 0) return
    const mark = `${selectedId}:${focusedMsgUid}`
    if (scrolledToRef.current === mark) return
    const idx = messages.findIndex((m) => m.uid === focusedMsgUid)
    if (idx === -1) return
    scrolledToRef.current = mark
    if (hasCollapsedTimeline && idx < messages.length - VISIBLE_RECENT) {
      setShowAllMessages(true)
    }
    setSelectedMsgIdx(idx)
    const el = document.getElementById(`msg-${focusedMsgUid}`)
    if (el) {
      window.setTimeout(() => el.scrollIntoView({ behavior: 'smooth', block: 'center' }), 120)
    }
  }, [focusedMsgUid, selectedId, messages, hasCollapsedTimeline])

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
  // The reading target, derived rather than stored. `selectedMsgIdx` is
  // "the message the reader clicked", and `null` there means "they have
  // not clicked one" — not "none is shown". A thread that has messages
  // always has one to show, so the pane's empty state is reachable only
  // when the thread is genuinely empty.
  const currentMsgIdx = selectedMsgIdx ?? defaultReadingTarget(messages, auth?.address ?? '')
  const selectedMsg = currentMsgIdx !== null ? messages[currentMsgIdx] : null

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

  const replyCtx: ThreadReplyContext = {
    forwardAttachmentsUid: fwdUid,
    forwardMessageId: fwdMessageId,
    lastMessageId: fwdLastMessageId,
    originalBody: fwdOriginalBody,
    originalDate: fwdOriginalDate,
    originalFrom: fwdOriginalFrom,
    originalHtmlBody: fwdOriginalHtml,
    replyAllRecipients: replyAllRecipients || extractEmail(messages[0]?.sender ?? ''),
    replyRecipients: replyRecipients || extractEmail(messages[0]?.sender ?? ''),
    subject: fwdSubject,
    threadId: selectedId,
  }

  return (
    <MPaneGroup>
      <ThreadContentPane
        auth={auth}
        contentScrollRef={contentScrollRef}
        goToNext={goToNext}
        goToPrev={goToPrev}
        handleForwardMsg={handleForwardMsg}
        handleMarkRead={handleMarkRead}
        handleMarkUnread={handleMarkUnread}
        handleReplyMsg={handleReplyMsg}
        handleStar={handleStar}
        handleUnstar={handleUnstar}
        hasNext={hasNext}
        hasPrev={hasPrev}
        isFlagged={isFlagged}
        isRead={isRead}
        loadingThread={loadingThread}
        messages={messages}
        mobileThreadTab={mobileThreadTab}
        myEmail={myEmail}
        onBack={onBack}
        selectedMsg={selectedMsg}
        selectedMsgIdx={currentMsgIdx}
        setMobileReplyOpen={setMobileReplyOpen}
        setMobileThreadTab={setMobileThreadTab}
        setShowDeleteConfirm={setShowDeleteConfirm}
        setTimelineCollapsed={setTimelineCollapsed}
        subject={subject}
        timelineCollapsed={timelineCollapsed}
      />
      <ThreadConversationPane
        bottomRef={bottomRef}
        displayName={auth?.display_name}
        handleSelectMsg={handleSelectMsg}
        hasCollapsedTimeline={hasCollapsedTimeline}
        loadingThread={loadingThread}
        messages={messages}
        mobileThreadTab={mobileThreadTab}
        myEmail={myEmail}
        refetchThread={refetchThread}
        replyCtx={replyCtx}
        replyMode={replyMode}
        selectedMsgIdx={currentMsgIdx}
        setForwardSource={setForwardSource}
        setMobileThreadTab={setMobileThreadTab}
        setReplyMode={setReplyMode}
        setShowAllMessages={setShowAllMessages}
        timelineCollapsed={timelineCollapsed}
      />
      <ThreadViewDialogs
        handleDelete={handleDelete}
        mobileReplyOpen={mobileReplyOpen}
        refetchThread={refetchThread}
        replyCtx={replyCtx}
        replyMode={replyMode}
        setForwardSource={setForwardSource}
        setMobileReplyOpen={setMobileReplyOpen}
        setReplyMode={setReplyMode}
        setShowDeleteConfirm={setShowDeleteConfirm}
        showDeleteConfirm={showDeleteConfirm}
        subject={subject}
      />
    </MPaneGroup>
  )
}
