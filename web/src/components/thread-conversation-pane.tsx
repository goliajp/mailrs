import type { ReplyMode } from '@/components/reply-box'
import type { ThreadReplyContext } from '@/components/thread-reply-box'
import type { ThreadMessage } from '@/lib/types'
import type { MobileThreadTab } from '@/store/ui'
import type { RefObject } from 'react'

import { ArrowLeft } from 'lucide-react'
import { useMemo } from 'react'

import { ThreadReplyBox } from '@/components/thread-reply-box'
import { ThreadTimelineItem } from '@/components/thread-view-bubble'
import { bubbleDateLabel } from '@/components/thread-view-helpers'
import { MPane } from '@/layouts/pane'
import { extractEmail, extractName } from '@/lib/avatar'
import { isOwnMessage } from '@/lib/sender-identity'

// how many messages stay visible when a long thread is collapsed.
export const VISIBLE_RECENT = 3

type ConversationPaneProps = {
  bottomRef: RefObject<HTMLDivElement | null>
  displayName?: string
  handleSelectMsg: (idx: number) => void
  hasCollapsedTimeline: boolean
  loadingThread: boolean
  messages: readonly ThreadMessage[]
  mobileThreadTab: MobileThreadTab
  myEmail: string
  refetchThread: () => void
  replyCtx: ThreadReplyContext
  replyMode: ReplyMode
  selectedMsgIdx: null | number
  setForwardSource: (v: null) => void
  setMobileThreadTab: (v: MobileThreadTab) => void
  setReplyMode: (v: ReplyMode) => void
  setShowAllMessages: (v: boolean) => void
  timelineCollapsed: boolean
}

// the right-hand panel: the message timeline plus the desktop composer.
export function ThreadConversationPane({
  bottomRef,
  displayName,
  handleSelectMsg,
  hasCollapsedTimeline,
  loadingThread,
  messages,
  mobileThreadTab,
  myEmail,
  refetchThread,
  replyCtx,
  replyMode,
  selectedMsgIdx,
  setForwardSource,
  setMobileThreadTab,
  setReplyMode,
  setShowAllMessages,
  timelineCollapsed,
}: ConversationPaneProps) {
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
      // Not `senderEmail === myEmail`: a forged From carrying your own
      // address rendered as "Me" and, because the badge is drawn only for
      // messages that are not yours, suppressed the one thing that would
      // have given it away.
      const isOwn = isOwnMessage(senderEmail, myEmail, msg.sender_trust)
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
  }, [messages, myEmail, hasCollapsedTimeline])

  return (
    <>
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
                    myName={displayName}
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
            <ThreadReplyBox
              ctx={replyCtx}
              mode={replyMode}
              onModeChange={(m) => {
                setReplyMode(m)
                if (m !== 'forward') setForwardSource(null)
              }}
              onSent={() => {
                setForwardSource(null)
                refetchThread()
              }}
            />
          </div>
        </div>
      </MPane>
    </>
  )
}
