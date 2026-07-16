import type { WireSentMessage } from '@/wire/schemas/mail'

import { useSetAtom } from 'jotai'

import { FilterBar } from '@/components/conversation-list-filter-bar'
import { useSentMessagesQuery } from '@/hooks/use-sent-messages'
import { extractEmail, extractName } from '@/lib/avatar'
import { formatFullDate } from '@/lib/format'
import { focusedMessageUidAtom, mobileViewAtom, selectedThreadIdAtom } from '@/store/ui'

// per-message Sent view: one row per outbound message (not per thread),
// showing the recipient. clicking opens the thread and focuses this exact
// message. Renders the FilterBar so tab navigation stays available (like
// DraftsList — it replaces ConversationList wholesale).
export function SentList() {
  const { data: messages = [], isLoading } = useSentMessagesQuery()
  const setSelectedThreadId = useSetAtom(selectedThreadIdAtom)
  const setFocusedMsgUid = useSetAtom(focusedMessageUidAtom)
  const setMobileView = useSetAtom(mobileViewAtom)

  const openMessage = (m: WireSentMessage) => {
    setSelectedThreadId(m.thread_id)
    setFocusedMsgUid(m.uid)
    setMobileView('thread')
  }

  const renderBody = () => {
    if (isLoading) {
      return <div className="text-fg-muted p-4 text-xs">Loading…</div>
    }
    if (messages.length === 0) {
      return <div className="text-fg-muted p-8 text-center text-sm">No sent messages</div>
    }
    return (
      <div className="flex flex-col">
        {messages.map((m) => (
          <button
            className="border-border hover:bg-bg-secondary flex flex-col gap-1 border-b px-4 py-3 text-left transition-colors"
            key={m.uid}
            onClick={() => openMessage(m)}
            type="button"
          >
            <div className="flex items-center justify-between gap-2">
              <span className="text-accent truncate text-sm font-semibold">
                To: {recipientLabel(m.to)}
              </span>
              <span className="text-fg-muted text-tiny shrink-0">
                {formatFullDate(m.internal_date)}
              </span>
            </div>
            <span className="text-fg truncate text-sm">{subjectLabel(m.subject)}</span>
          </button>
        ))}
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <FilterBar />
      <div className="min-h-0 flex-1 overflow-y-auto">{renderBody()}</div>
    </div>
  )
}

// the raw To header can be "Name <a@x>, b@y, …" — show the first
// recipient's name/email and a "+N" for the rest.
function recipientLabel(to: string): string {
  const parts = to
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
  if (parts.length === 0) return '—'
  const first = extractName(parts[0]) || extractEmail(parts[0]) || parts[0]
  if (parts.length === 1) return first
  return `${first} +${parts.length - 1}`
}

function subjectLabel(subject: string): string {
  if (subject.trim()) return subject
  return '(no subject)'
}
