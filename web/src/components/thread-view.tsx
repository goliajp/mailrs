import type { ThreadMessage } from '@/lib/types'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { Mail } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { DeleteThreadConfirm } from '@/components/delete-thread-confirm'
import { type ReplyMode } from '@/components/reply-box'
import { type ThreadReplyContext } from '@/components/thread-reply-box'
import { ThreadStack } from '@/components/thread-stack/thread-stack'
import { useConversationActions } from '@/hooks/use-conversation-actions'
import {
  useConversationRows,
  useCurrentListRows,
  useCurrentSelection,
  useSelectThreadId,
} from '@/hooks/use-current-list'
import { useThreadQuery } from '@/hooks/use-mail-queries'
import { useMarkReadOnOpen } from '@/hooks/use-mark-read-on-open'
import { useThreadActions } from '@/hooks/use-thread-actions'
import { useT } from '@/i18n'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { toggle } from '@/lib/actions'
import { extractEmail } from '@/lib/avatar'
import { formatFullDate } from '@/lib/format'
import { splitAddressList } from '@/lib/recipients'
import { authAtom } from '@/store/auth'
import {
  composeReplySourceAtom,
  composingNewAtom,
  selectedDomainsAtom,
  threadShortcutAtom,
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

export function ThreadView() {
  const t = useT()
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
  // Prev/next walk the same rows the list draws, because they are the
  // same value — this used to read an atom the list kept in step with an
  // effect, which is two copies of one list.
  const rows = useCurrentListRows()
  const setSelectedId = useSelectThreadId()
  // Which mailbox this conversation arrived at, so a reply leaves by
  // the same door it came in.
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
  const contentScrollRef = useRef<HTMLDivElement>(null)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
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
  // Tracks the threadId we've already auto-marked-read this entry. Set on
  // first run for a given selectedId; cleared when selectedId changes. Any
  // thread messages now live in react-query; we bridge to the legacy
  // threadMessagesAtom for downstream consumers. The bridge is structured
  // to eliminate the thread-switch flash: while a new thread is fetching
  // (selectedId already pointed at B but threadQuery.data still resolving),
  // we leave `messages` AS-IS — i.e. keep displaying
  // the previous thread — and swap atomically when the new data arrives.
  // No eager reset means no intermediate empty state, no flicker.
  const threadQuery = useThreadQuery(selectedId, selectedDomains)
  const loadingThread = threadQuery.isPending && !!selectedId
  // `messages` is the RQ query result directly — no mirror, no bridge,
  // no accumulation path. `EMPTY_MESSAGES` is a stable reference so
  // `React.memo`d children with array-typed props keep identity across
  // loading frames.
  const messages: readonly ThreadMessage[] = threadQuery.data ?? EMPTY_MESSAGES

  // When a conversation's messages first arrive, bring its first open
  // message to the top: the reply box is at the end of the stack, so the
  // bottom would show the box and not what is to be read. Latched per
  // thread: scrolling is an action, not a state.
  const scrolledFor = useRef<null | string>(null)
  useEffect(() => {
    if (!selectedId || !threadQuery.data) return
    if (scrolledFor.current === selectedId) return
    scrolledFor.current = selectedId
    requestAnimationFrame(() => {
      contentScrollRef.current
        ?.querySelector('[data-stack-header][aria-expanded="true"]')
        ?.scrollIntoView?.({ behavior: 'instant', block: 'start' })
    })
  }, [threadQuery.data, selectedId])

  // invalidate the active thread (used after Reply / Forward send so the
  // new outbound message shows up immediately)
  const {
    handleDelete,
    handleMarkRead,
    handleMarkUnread,
    handleStar,
    handleUnstar,
    refetchThread,
  } = useThreadActions({
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
      setShowDeleteConfirm(false)
      setForwardSource(null)
      return
    }
    setForwardSource(null)
    setReplyMode('reply')
    // v2.1 phase-5b: imperative lookup used to read
    // `store.get(conversationsAtom).find(...)`. Now walks the RQ cache
    // directly — one flatten over the current `conversationKeys.infinites`
    // entries. Latest patch (mark-read etc.) is reflected without needing
    // the atom-sync effect in chat.tsx to run first.
    const existing = currentConversations.find((c) => c.thread_id === selectedId)
    setIsRead(!existing || existing.unread_count === 0)
    setIsFlagged(existing?.flagged ?? false)
    // thread fetch is owned by useThreadQuery; nothing imperative to do here
  }, [selectedId, currentConversations])

  // archiving runs through the same dispatcher as the list's row, so the
  // toast, its undo and `z` behave alike
  const { act } = useConversationActions()
  const selectedRow = currentConversations.find((c) => c.thread_id === selectedId)
  const isArchived = selectedRow?.archived ?? false
  const handleArchive = useCallback(() => {
    if (!selectedId) return
    void act(selectedId, toggle(isArchived, 'unarchive', 'archive'))
  }, [act, isArchived, selectedId])
  // deleting goes to the Trash without asking; in the Trash it is for
  // good, and asked first
  const isTrashed = (selectedRow?.trashed_at ?? 0) > 0
  const handleTrash = useCallback(() => {
    if (!selectedId) return
    if (isTrashed) {
      setShowDeleteConfirm(true)
      return
    }
    void act(selectedId, 'trash')
  }, [act, isTrashed, selectedId])
  const handleUntrash = useCallback(() => {
    if (!selectedId) return
    void act(selectedId, 'untrash')
  }, [act, selectedId])

  // `r`, `f` and `#` from the keyboard do what this pane's own buttons
  // do, on the newest message, once the thread they name is loaded.
  // After the reset above, which would otherwise undo a forward when
  // both run in one commit
  const [shortcut, setShortcut] = useAtom(threadShortcutAtom)
  useEffect(() => {
    if (!shortcut || shortcut.threadId !== selectedId || messages.length === 0) return
    setShortcut(null)
    const newest = messages[messages.length - 1]
    switch (shortcut.kind) {
      case 'delete':
        handleTrash()
        break
      case 'forward':
        handleForwardMsg(newest)
        break
      case 'reply':
        handleReplyMsg(newest)
        break
    }
  }, [shortcut, selectedId, messages, setShortcut, handleForwardMsg, handleReplyMsg, handleTrash])

  // Showing is unconditionally true here: this component only renders
  // when the desktop reading pane is on screen. The mobile view passes
  // its own answer — see the hook for why "selected" is not the same
  // question.
  useMarkReadOnOpen(selectedId, true, () => setIsRead(true))

  // A message that arrives while the conversation is open is scrolled to.
  // Keyed on the newest uid, not the array: a refetch that brings nothing
  // new hands back a new array and must not scroll.
  const lastMessageUid = messages[messages.length - 1]?.uid
  const arrivedFor = useRef<null | string>(null)
  useEffect(() => {
    if (lastMessageUid === undefined || !selectedId) return
    const mark = `${selectedId}:${lastMessageUid}`
    if (arrivedFor.current === null || !arrivedFor.current.startsWith(`${selectedId}:`)) {
      arrivedFor.current = mark
      return
    }
    if (arrivedFor.current === mark) return
    arrivedFor.current = mark
    document
      .getElementById(`msg-${lastMessageUid}`)
      ?.scrollIntoView?.({ behavior: 'smooth', block: 'start' })
  }, [lastMessageUid, selectedId])

  // A row that names one message (a Send row) scrolls to it; the stack
  // opens it. Scrolling is an action, so it is guarded on the pair it last
  // ran for rather than by clearing the value that asked for it.
  const scrolledToRef = useRef<null | string>(null)
  useEffect(() => {
    if (focusedMsgUid === null || selectedId === null || messages.length === 0) return
    const mark = `${selectedId}:${focusedMsgUid}`
    if (scrolledToRef.current === mark) return
    if (!messages.some((m) => m.uid === focusedMsgUid)) return
    scrolledToRef.current = mark
    window.setTimeout(() => {
      document
        .getElementById(`msg-${focusedMsgUid}`)
        ?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    }, 120)
  }, [focusedMsgUid, selectedId, messages])

  // empty state
  if (!selectedId) {
    return (
      <MPane center>
        <div className="text-fg-muted text-center">
          <Mail className="mx-auto mb-3 h-10 w-10" strokeWidth={1.5} />
          <p className="text-sm font-medium">{t('read.noneSelected')}</p>
          <p className="mt-1 text-xs">{t('read.noneSelectedHint')}</p>
        </div>
      </MPane>
    )
  }

  const subject = messages[0]?.subject ?? ''
  const lastMsg = messages[messages.length - 1]
  const myEmail = auth?.address ?? ''
  const replyRecipients = lastMsg ? extractEmail(lastMsg.sender) : ''
  const replyAllRecipients = lastMsg
    ? (() => {
        const senderEmail = extractEmail(lastMsg.sender)
        const recipientEmails = splitAddressList(lastMsg.recipients).map(extractEmail)
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
  const fwdMsg = forwardSource ? null : lastMsg
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
    originalTo: lastMsg?.recipients ?? '',
    replyAllRecipients: replyAllRecipients || extractEmail(messages[0]?.sender ?? ''),
    replyRecipients: replyRecipients || extractEmail(messages[0]?.sender ?? ''),
    subject: fwdSubject,
    threadId: selectedId,
  }

  return (
    <MPaneGroup>
      <ThreadStack
        controls={{
          goToNext,
          goToPrev,
          hasNext,
          hasPrev,
          isArchived,
          isFlagged,
          isRead,
          isTrashed,
          onArchive: handleArchive,
          onMarkRead: handleMarkRead,
          onMarkUnread: handleMarkUnread,
          onStar: handleStar,
          onTrash: handleTrash,
          onUnstar: handleUnstar,
          onUntrash: handleUntrash,
        }}
        focusedUid={focusedMsgUid}
        loadingThread={loadingThread}
        messages={messages}
        myEmail={myEmail}
        myName={auth?.display_name}
        onForward={handleForwardMsg}
        onReply={handleReplyMsg}
        onReplyModeChange={(m) => {
          setReplyMode(m)
          if (m !== 'forward') setForwardSource(null)
        }}
        onReplySent={() => {
          setForwardSource(null)
          refetchThread()
        }}
        replyCtx={replyCtx}
        replyMode={replyMode}
        scrollRef={contentScrollRef}
        subject={subject}
        threadId={selectedId}
      />
      <DeleteThreadConfirm
        onCancel={() => setShowDeleteConfirm(false)}
        onConfirm={handleDelete}
        open={showDeleteConfirm}
      />
    </MPaneGroup>
  )
}
