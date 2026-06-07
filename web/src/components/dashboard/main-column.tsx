import type { ConversationSummary } from '@/lib/types'

import { AlertTriangle, Clock, Mail, Pin, Plus, Shield } from 'lucide-react'

import { ConversationRow } from './conversation-row'
import { Section } from './section'

type MainColumnProps = {
  conversations: ConversationSummary[]
  needsAttention: ConversationSummary[]
  onCompose: () => void
  onOpenInbox: () => void
  onOpenThread: (threadId: string) => void
  pinned: ConversationSummary[]
  recentUnread: ConversationSummary[]
  totalUnread: number
}

export function MainColumn({
  conversations,
  needsAttention,
  onCompose,
  onOpenInbox,
  onOpenThread,
  pinned,
  recentUnread,
  totalUnread,
}: MainColumnProps) {
  const allEmpty = pinned.length === 0 && needsAttention.length === 0 && recentUnread.length === 0
  const handleEmptyAction = totalUnread > 0 ? onOpenInbox : onCompose

  return (
    <div className="min-w-0 space-y-6 lg:col-span-2">
      {pinned.length > 0 && (
        <Section icon={Pin} title="Pinned">
          <div className="space-y-0.5">
            {pinned.map((c) => (
              <ConversationRow
                conversation={c}
                key={c.thread_id}
                onClick={() => onOpenThread(c.thread_id)}
              />
            ))}
          </div>
        </Section>
      )}

      {needsAttention.length > 0 && (
        <Section
          action={{ label: 'View all', onClick: onOpenInbox }}
          icon={AlertTriangle}
          title="Needs Attention"
        >
          <div className="space-y-0.5">
            {needsAttention.map((c) => (
              <ConversationRow
                conversation={c}
                key={c.thread_id}
                onClick={() => onOpenThread(c.thread_id)}
              />
            ))}
          </div>
        </Section>
      )}

      {recentUnread.length > 0 && (
        <Section action={{ label: 'Open inbox', onClick: onOpenInbox }} icon={Mail} title="Recent">
          <div className="space-y-0.5">
            {recentUnread.map((c) => (
              <ConversationRow
                conversation={c}
                key={c.thread_id}
                onClick={() => onOpenThread(c.thread_id)}
              />
            ))}
          </div>
        </Section>
      )}

      {allEmpty && (
        <>
          <Section icon={Mail} title="Inbox">
            <div className="text-fg-muted flex flex-col items-center gap-2 py-6">
              <Shield aria-hidden="true" className="h-8 w-8" />
              <p className="text-sm">
                {totalUnread > 0
                  ? `${totalUnread} unread further down — open inbox to see`
                  : 'All caught up — no unread emails'}
              </p>
              <button
                className="bg-accent/10 text-accent hover:bg-accent mt-2 flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:text-white"
                onClick={handleEmptyAction}
                type="button"
              >
                {totalUnread > 0 ? (
                  <>
                    <Mail className="h-3.5 w-3.5" />
                    Open inbox
                  </>
                ) : (
                  <>
                    <Plus className="h-3.5 w-3.5" />
                    Compose new email
                  </>
                )}
              </button>
            </div>
          </Section>
          {conversations.length > 0 && (
            <Section
              action={{ label: 'Open inbox', onClick: onOpenInbox }}
              icon={Clock}
              title="Recent Activity"
            >
              <div className="space-y-0.5">
                {conversations.slice(0, 5).map((c) => (
                  <ConversationRow
                    conversation={c}
                    key={c.thread_id}
                    onClick={() => onOpenThread(c.thread_id)}
                  />
                ))}
              </div>
            </Section>
          )}
        </>
      )}
    </div>
  )
}
