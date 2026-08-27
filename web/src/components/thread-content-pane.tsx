import type { ThreadMessage } from '@/lib/types'
import type { MobileThreadTab } from '@/store/ui'
import type { Dispatch, RefObject, SetStateAction } from 'react'

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
} from 'lucide-react'
import { useMemo } from 'react'

import { AiAnalysisPanel } from '@/components/ai-analysis'
import { AttachmentPreview } from '@/components/attachment-preview'
import { Copyable } from '@/components/copy-button'
import { InviteCard } from '@/components/invite-card'
import { linkifyNodes } from '@/components/linkify-nodes'
import { MessageBubble } from '@/components/message-bubble'
import { MessageSource } from '@/components/message-source'
import { SenderAvatar } from '@/components/sender-avatar'
import { SenderClaimBadge } from '@/components/sender-claim-badge'
import { SenderTrustBadge } from '@/components/sender-trust-badge'
import { StructuredDataCard } from '@/components/structured-data-card'
import { FeedbackMenu, HdrBtn, SmBtn } from '@/components/thread-view-bubble'
import { formatRecipients } from '@/components/thread-view-helpers'
import { UnsubscribeFooter } from '@/components/unsubscribe-footer'
import { useSelectedThreadId } from '@/hooks/use-current-list'
import { MPane } from '@/layouts/pane'
import { extractEmail, extractName } from '@/lib/avatar'
import { htmlBodyPaintsNothing } from '@/lib/email-split'
import { formatFullDate } from '@/lib/format'
import { highlightMentions } from '@/lib/mention'
import { downloadEml, printMessage } from '@/lib/message-export'
import { isOwnMessage, isSpoofSuspected } from '@/lib/sender-identity'

const EMPTY_ATTACHMENTS: never[] = []

type ContentPaneProps = {
  auth: null | { display_name?: string }
  contentScrollRef: RefObject<HTMLDivElement | null>
  goToNext: () => void
  goToPrev: () => void
  handleForwardMsg: (msg: ThreadMessage) => void
  handleMarkRead: () => void
  handleMarkUnread: () => void
  handleReplyMsg: (msg: ThreadMessage) => void
  handleStar: () => void
  handleUnstar: () => void
  hasNext: boolean
  hasPrev: boolean
  isFlagged: boolean
  isRead: boolean
  loadingThread: boolean
  messages: readonly ThreadMessage[]
  mobileThreadTab: MobileThreadTab
  myEmail: string
  onBack?: () => void
  selectedMsg: null | ThreadMessage | undefined
  selectedMsgIdx: null | number
  setMobileReplyOpen: (v: boolean) => void
  setMobileThreadTab: (v: MobileThreadTab) => void
  setShowDeleteConfirm: (v: boolean) => void
  setTimelineCollapsed: Dispatch<SetStateAction<boolean>>
  subject: string
  timelineCollapsed: boolean
}

// the left/main panel: thread header bar, the selected message, and the
// per-message actions. split out of ThreadView on 2026-08-02 purely by
// location — nothing here changed.
export function ThreadContentPane({
  auth,
  contentScrollRef,
  goToNext,
  goToPrev,
  handleForwardMsg,
  handleMarkRead,
  handleMarkUnread,
  handleReplyMsg,
  handleStar,
  handleUnstar,
  hasNext,
  hasPrev,
  isFlagged,
  isRead,
  loadingThread,
  messages,
  mobileThreadTab,
  myEmail,
  onBack,
  selectedMsg,
  selectedMsgIdx,
  setMobileReplyOpen,
  setMobileThreadTab,
  setShowDeleteConfirm,
  setTimelineCollapsed,
  subject,
  timelineCollapsed,
}: ContentPaneProps) {
  // The thread this pane is showing — `POST /api/mail/unsubscribe`
  // names the message by (thread, uid) rather than by URL, which is
  // what stops it being a request forwarder.
  const threadId = useSelectedThreadId()
  // A non-empty `html_body` is not the same as a message with a body. The
  // mailing that surfaced this carried 2.4 kB of stylesheet, a hidden
  // preheader and a tracking gif, and rendered as a white box — which
  // reads as a failure to load rather than as an empty message. When the
  // HTML would paint nothing, show the text part, which at least holds
  // whatever the preheader said.
  const html = selectedMsg?.html_body
  const showHtml = useMemo(() => !!html && !htmlBodyPaintsNothing(html), [html])
  return (
    <>
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
          <div className="flex shrink items-center gap-0.5 lg:gap-1">
            {/* Hidden below `lg`, where the pane can be 280px and
                seven buttons leave the subject about sixty. `j`/`k`
                and the list itself already do this. */}
            <HdrBtn
              className="hidden lg:inline-flex"
              disabled={!hasPrev}
              onClick={goToPrev}
              title="Previous conversation"
            >
              <ChevronUp className="h-4 w-4" />
            </HdrBtn>
            <HdrBtn
              className="hidden lg:inline-flex"
              disabled={!hasNext}
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
            {/* The "Close" X is gone. Deselecting is no longer a state
                the model has: a list with rows always has a current one,
                so clearing the pick only jumped you back to the first
                row. Use the list to move, or the collapse button above
                to get the pane out of the way. */}
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
          <div
            className="divide-border min-w-0 flex-1 divide-y overflow-y-auto"
            data-selectable
            ref={contentScrollRef}
          >
            {selectedMsg ? (
              <>
                {/* Email header (sender info). Each of the four info rows
                    has a locked height with vertical-centered content so
                    the block's total height is constant regardless of which
                    optional badges are present — switching between messages
                    no longer shifts the body downward.
                    Tags below use inline-flex h-4 leading-none so their
                    padding can't add vertical space beyond the row's box. */}
                <div className="shrink-0 px-4 py-2">
                  <div className="flex items-start gap-2.5">
                    {/* A spoof wearing your address would otherwise be
                        drawn with your own avatar, which is the most
                        convincing part of it. */}
                    <SenderAvatar
                      className="mt-0.5"
                      sender={isSpoofSuspected(selectedMsg.sender_trust) ? '' : selectedMsg.sender}
                      size={28}
                    />
                    <div className="min-w-0 flex-1 space-y-0.5">
                      <div className="flex h-5 items-center justify-between gap-2">
                        <p
                          className={`flex h-5 items-center text-sm font-medium select-text ${
                            isOwnMessage(
                              extractEmail(selectedMsg.sender),
                              myEmail,
                              selectedMsg.sender_trust
                            )
                              ? 'text-accent'
                              : 'text-fg'
                          }`}
                        >
                          <span className="truncate">
                            {isOwnMessage(
                              extractEmail(selectedMsg.sender),
                              myEmail,
                              selectedMsg.sender_trust
                            )
                              ? 'Me'
                              : extractName(selectedMsg.sender)}
                          </span>
                          {/* The reading pane is where a message is actually
                              read, and it showed no verdict at all — the
                              badge existed only in the timeline bubbles. */}
                          <span className="ml-1.5 flex shrink-0 items-center gap-1">
                            <SenderTrustBadge trust={selectedMsg.sender_trust} />
                            {/* The name says one domain and the wire says
                                another — the gap brand impersonation lives
                                in, and one no authentication check has an
                                opinion about. */}
                            <SenderClaimBadge sender={selectedMsg.sender} />
                          </span>
                        </p>
                        <div className="flex h-5 shrink-0 items-center gap-0.5">
                          <SmBtn onClick={() => handleReplyMsg(selectedMsg)} title="Reply">
                            <Reply className="h-3.5 w-3.5" />
                          </SmBtn>
                          <SmBtn onClick={() => handleForwardMsg(selectedMsg)} title="Forward">
                            <Forward className="h-3.5 w-3.5" />
                          </SmBtn>
                          <SmBtn onClick={() => printMessage(selectedMsg)} title="Print">
                            <Printer className="h-3.5 w-3.5" />
                          </SmBtn>
                          <SmBtn
                            onClick={() => downloadEml(selectedMsg.uid, selectedMsg.subject)}
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
                {/* No wrapper padding: the card is already a bordered,
                    rounded area with its own `p-4` and `my-3`. A second
                    frame around it draws the same box twice. */}
                {selectedMsg.invite_method && <InviteCard messageUid={selectedMsg.uid} />}

                {/* email body */}
                {showHtml && (
                  <div>
                    <MessageBubble
                      attachments={EMPTY_ATTACHMENTS}
                      htmlBody={selectedMsg.html_body}
                      isOwn={false}
                      textBody={null}
                      uid={selectedMsg.uid}
                    />
                    <div className="px-4 pb-3">
                      <MessageSource uid={selectedMsg.uid} />
                      <UnsubscribeFooter
                        header={selectedMsg.unsubscribe}
                        threadId={threadId ?? ''}
                        uid={selectedMsg.uid}
                      />
                    </div>
                  </div>
                )}
                {!showHtml && (
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
                {/* The whole html, not the split body: a forward's
                    inline images are referenced from the quoted half. */}
                <AttachmentPreview
                  attachments={selectedMsg.attachments}
                  html={selectedMsg.html_body}
                  uid={selectedMsg.uid}
                />
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
    </>
  )
}
