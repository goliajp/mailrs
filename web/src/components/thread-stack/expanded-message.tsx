import type { ThreadMessage } from '@/lib/types'

import { Download, Forward, Printer, Reply } from 'lucide-react'
import { useMemo } from 'react'

import { AiAnalysisPanel } from '@/components/ai-analysis'
import { AttachmentPreview } from '@/components/attachment-preview'
import { CallbackNumberNotice } from '@/components/callback-number-notice'
import { Copyable } from '@/components/copy-button'
import { FraudVerdictPanel } from '@/components/fraud-verdict-panel'
import { InviteCard } from '@/components/invite-card'
import { linkifyNodes } from '@/components/linkify-nodes'
import { MessageBubble } from '@/components/message-bubble'
import { MessageSource } from '@/components/message-source'
import { MessageRecipients } from '@/components/recipient-list'
import { SenderAvatar } from '@/components/sender-avatar'
import { SenderClaimBadge } from '@/components/sender-claim-badge'
import { SenderChecks, SenderTrustBadge } from '@/components/sender-trust-badge'
import { StructuredDataCard } from '@/components/structured-data-card'
import { FeedbackMenu, SmBtn } from '@/components/thread-view-bubble'
import { UnsubscribeFooter } from '@/components/unsubscribe-footer'
import { type Key, useT } from '@/i18n'
import { extractEmail, extractName } from '@/lib/avatar'
import { htmlBodyPaintsNothing } from '@/lib/email-split'
import { formatFullDate } from '@/lib/format'
import { highlightMentions } from '@/lib/mention'
import { downloadEml, printMessage } from '@/lib/message-export'
import { ownName } from '@/lib/own-name'
import { isOwnMessage, isSpoofSuspected, verifiedSender } from '@/lib/sender-identity'

const EMPTY_ATTACHMENTS: never[] = []

type Props = {
  /** Every message of the conversation, this one among them. */
  conversation: readonly ThreadMessage[]
  message: ThreadMessage
  myEmail: string
  myName?: string
  onForward: (m: ThreadMessage) => void
  onReply: (m: ThreadMessage) => void
  onToggle: () => void
  threadId: string
}

/** One message of the stack, open: who sent it, why it was held, and all of it. */
export function ExpandedMessage({
  conversation,
  message: m,
  myEmail,
  myName,
  onForward,
  onReply,
  onToggle,
  threadId,
}: Props) {
  const t = useT()
  const own = isOwnMessage(extractEmail(m.sender), myEmail, m.sender_trust)
  // A non-empty `html_body` is not the same as a message with a body: one
  // mailing carried only a stylesheet, a hidden preheader and a tracking
  // gif, and painted a white box. Then the text part is shown instead.
  const html = m.html_body
  const showHtml = useMemo(() => !!html && !htmlBodyPaintsNothing(html), [html])
  return (
    <article className="divide-border divide-y" data-selectable id={`msg-${m.uid}`}>
      <div className="px-4 py-2">
        {/* In a narrow reading pane the actions drop to their own line
            rather than squeezing the sender and date to nothing: the
            column asks for 14rem before it shares the row. */}
        <div className="flex flex-wrap items-start gap-x-2 gap-y-1">
          <button
            aria-expanded
            className="focus-visible:ring-accent/50 flex min-w-0 flex-[1_1_14rem] items-start gap-2.5 rounded-md text-left focus-visible:ring-2 focus-visible:outline-none"
            data-stack-header
            onClick={onToggle}
            type="button"
          >
            {/* A spoof wearing your address would otherwise be drawn with
                your own avatar, its most convincing part. */}
            <SenderAvatar
              className="mt-0.5"
              sender={isSpoofSuspected(m.sender_trust) ? '' : m.sender}
              size={28}
            />
            <span className="min-w-0 flex-1">
              <span className={`flex h-5 items-center text-sm font-medium ${nameClass(own)}`}>
                <span className="truncate">{ownName(own, extractName(m.sender))}</span>
                <span className="ml-1.5 flex shrink-0 items-center gap-1">
                  <SenderTrustBadge trust={m.sender_trust} />
                  <SenderClaimBadge sender={m.sender} />
                </span>
              </span>
              <span className="text-fg-muted block text-xs break-keep" data-message-date>
                {formatFullDate(m.internal_date)}
              </span>
            </span>
          </button>
          <div className="ml-auto flex h-5 shrink-0 items-center gap-0.5">
            <SmBtn onClick={() => onReply(m)} title={t('read.reply')}>
              <Reply className="h-3.5 w-3.5" />
            </SmBtn>
            <SmBtn onClick={() => onForward(m)} title={t('read.forward')}>
              <Forward className="h-3.5 w-3.5" />
            </SmBtn>
            <SmBtn onClick={() => printMessage(m)} title={t('read.print')}>
              <Printer className="h-3.5 w-3.5" />
            </SmBtn>
            <SmBtn onClick={() => downloadEml(m.uid, m.subject)} title={t('read.downloadEml')}>
              <Download className="h-3.5 w-3.5" />
            </SmBtn>
            <FeedbackMenu senderEmail={extractEmail(m.sender)} />
          </div>
        </div>
        <div className="text-fg-muted mt-0.5 space-y-0.5 pl-[38px] text-xs select-text">
          <p className="flex h-4 items-center">
            <Copyable value={extractEmail(m.sender)}>
              <span className="truncate">{extractEmail(m.sender)}</span>
            </Copyable>
          </p>
          <MessageRecipients cc={m.cc ?? ''} myEmail={myEmail} to={m.recipients} />
          {isSpoofSuspected(m.sender_trust) && <SenderChecks checks={m.sender_checks} />}
          <MessageTags message={m} />
        </div>
      </div>

      {/* why it was held, above the body: a reader should know what they
          are looking at before they read it */}
      <FraudVerdictPanel messageId={m.message_id} threadId={threadId} />
      {!own && <CallbackNumberNotice conversation={conversation} message={m} />}
      {m.structured_data && <StructuredDataCard data={m.structured_data} />}
      <AiAnalysisPanel message={m} />
      {m.invite_method && <InviteCard messageUid={m.uid} />}

      {showHtml && (
        <div>
          <MessageBubble
            attachments={EMPTY_ATTACHMENTS}
            htmlBody={m.html_body}
            imagesFrom={verifiedSender(m)}
            imagesOf={threadId}
            isOwn={false}
            linkWarnings={m.link_warnings}
            textBody={null}
            uid={m.uid}
          />
        </div>
      )}
      {!showHtml && (
        <div className="px-4 py-3 select-text">
          <div className="text-fg text-mid font-sans leading-relaxed break-words whitespace-pre-wrap">
            {linkifyNodes(
              highlightMentions(m.clean_text || m.text_body || t('read.noText'), myEmail, myName),
              'text-accent no-underline hover:underline'
            )}
          </div>
        </div>
      )}
      <div className="px-4 pb-3">
        <MessageSource uid={m.uid} />
        <UnsubscribeFooter header={m.unsubscribe} threadId={threadId} uid={m.uid} />
      </div>
      {/* the whole html, not the split body: a forward's inline images are
          referenced from the quoted half */}
      <AttachmentPreview attachments={m.attachments} html={m.html_body} uid={m.uid} />
    </article>
  )
}

function MessageTags({ message: m }: { message: ThreadMessage }) {
  const t = useT()
  if (!m.action_deadline && m.risk_score < 40) return null
  return (
    <p className="flex h-5 items-center gap-1.5">
      {m.action_deadline && (
        <span className="bg-warning/10 text-warning text-mini inline-flex h-4 items-center rounded px-1.5 leading-none font-medium">
          {t('read.due', { when: m.action_deadline })}
        </span>
      )}
      {m.risk_score >= 40 && (
        <span
          className={`text-mini inline-flex h-4 items-center rounded px-1.5 leading-none font-medium ${riskClass(m.risk_score)}`}
        >
          {t(riskLabel(m.risk_score))}
        </span>
      )}
    </p>
  )
}

function nameClass(own: boolean): string {
  if (own) return 'text-accent'
  return 'text-fg'
}

function riskClass(score: number): string {
  if (score >= 60) return 'bg-danger/10 text-danger'
  return 'bg-warning/10 text-warning'
}

function riskLabel(score: number): Key {
  if (score >= 60) return 'read.risk.dangerous'
  return 'read.risk.suspicious'
}
