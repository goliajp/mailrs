import type { ConversationControls } from './stack-header'
import type { ReplyMode } from '@/components/reply-box'
import type { ThreadReplyContext } from '@/components/thread-reply-box'
import type { ThreadMessage } from '@/lib/types'
import type { RefObject } from 'react'

import { Paperclip } from 'lucide-react'

import { SenderAvatar } from '@/components/sender-avatar'
import { ThreadReplyBox } from '@/components/thread-reply-box'
import { useT } from '@/i18n'
import { MPane } from '@/layouts/pane'
import { extractEmail, extractName } from '@/lib/avatar'
import { formatDate } from '@/lib/format'
import { previewPlaceholder, previewText } from '@/lib/message-preview'
import { ownName } from '@/lib/own-name'
import { isOwnMessage } from '@/lib/sender-identity'

import { ExpandedMessage } from './expanded-message'
import { StackHeader } from './stack-header'
import { stackKeyDown } from './stack-keys'
import { stackItems } from './stack-model'
import { useStackState } from './use-stack-state'

type Props = {
  controls: ConversationControls
  focusedUid: null | number
  loadingThread: boolean
  messages: readonly ThreadMessage[]
  myEmail: string
  myName?: string
  onForward: (m: ThreadMessage) => void
  onReply: (m: ThreadMessage) => void
  onReplyModeChange: (m: ReplyMode) => void
  onReplySent: () => void
  replyCtx: ThreadReplyContext
  replyMode: ReplyMode
  scrollRef: RefObject<HTMLDivElement | null>
  subject: string
  threadId: string
}

/**
 * The open conversation as one stack, oldest first: the newest and the
 * unread messages open, the rest one line each, and the reply at the end.
 */
export function ThreadStack(p: Props) {
  const t = useT()
  const { expanded, showAll, showingAll, toggle } = useStackState(
    p.threadId,
    p.messages,
    p.focusedUid
  )
  const items = stackItems(p.messages, expanded, showingAll)
  return (
    <MPane className="min-w-0 flex-1">
      <StackHeader
        controls={p.controls}
        count={p.messages.length}
        subject={p.subject}
        threadId={p.threadId}
      />
      <div className="relative min-h-0 flex-1 overflow-y-auto" ref={p.scrollRef}>
        {p.loadingThread && p.messages.length === 0 && <StackSkeleton />}
        <div
          aria-label={t('stack.messages')}
          className="divide-border divide-y"
          onKeyDown={stackKeyDown}
          role="group"
        >
          {items.map((item) => {
            if (item.kind === 'hidden') {
              return (
                <button
                  className="text-accent hover:bg-bg-secondary block w-full py-2 text-center text-xs font-medium"
                  key={`hidden-${item.at}`}
                  onClick={showAll}
                  type="button"
                >
                  {t('thread.showEarlier', { count: item.count })}
                </button>
              )
            }
            const m = item.message
            if (item.expanded) {
              return (
                <ExpandedMessage
                  conversation={p.messages}
                  key={m.uid}
                  message={m}
                  myEmail={p.myEmail}
                  myName={p.myName}
                  onForward={p.onForward}
                  onReply={p.onReply}
                  onToggle={() => toggle(m.uid)}
                  threadId={p.threadId}
                />
              )
            }
            return (
              <CollapsedMessage
                key={m.uid}
                message={m}
                myEmail={p.myEmail}
                onToggle={() => toggle(m.uid)}
                subject={p.subject}
              />
            )
          })}
        </div>
        <div className="border-border flex min-h-[280px] flex-col border-t">
          <ThreadReplyBox
            ctx={p.replyCtx}
            mode={p.replyMode}
            onModeChange={p.onReplyModeChange}
            onSent={p.onReplySent}
          />
        </div>
      </div>
    </MPane>
  )
}

function CollapsedMessage({
  message: m,
  myEmail,
  onToggle,
  subject,
}: {
  message: ThreadMessage
  myEmail: string
  onToggle: () => void
  subject: string
}) {
  const t = useT()
  const own = isOwnMessage(extractEmail(m.sender), myEmail, m.sender_trust)
  const preview = previewText(m, subject)
  return (
    <button
      aria-expanded={false}
      className="hover:bg-bg-secondary focus-visible:ring-accent/50 flex w-full items-center gap-2.5 px-4 py-2 text-left focus-visible:ring-2 focus-visible:outline-none focus-visible:ring-inset"
      data-stack-header
      id={`msg-${m.uid}`}
      onClick={onToggle}
      type="button"
    >
      <SenderAvatar sender={m.sender} size={24} />
      <span className={`w-36 shrink-0 truncate text-sm font-medium ${nameClass(own)}`}>
        {ownName(own, extractName(m.sender))}
      </span>
      <span className="text-fg-muted min-w-0 flex-1 truncate text-sm">
        {preview || t(previewPlaceholder(m))}
      </span>
      {m.attachments.length > 0 && <Paperclip aria-hidden className="text-fg-muted h-3 w-3" />}
      <span className="text-fg-muted shrink-0 text-xs">{formatDate(m.internal_date)}</span>
    </button>
  )
}

function nameClass(own: boolean): string {
  if (own) return 'text-accent'
  return 'text-fg'
}

function StackSkeleton() {
  return (
    <div className="animate-pulse space-y-4 p-4">
      {Array.from({ length: 4 }).map((_, i) => (
        <div className="flex gap-3" key={i}>
          <div className="bg-border h-7 w-7 shrink-0 rounded-full" />
          <div className="min-w-0 flex-1 space-y-2">
            <div className="bg-border h-3.5 w-24 rounded" />
            <div className="bg-border h-10 w-full rounded" />
          </div>
        </div>
      ))}
    </div>
  )
}
