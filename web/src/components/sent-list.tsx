import type { ContextMenuItem } from '@/components/context-menu'
import type { WireSentMessage } from '@/wire/schemas/mail'

import { toast } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { memo, useMemo, useState } from 'react'

import { ActionSheet, ContextMenu, useContextMenu } from '@/components/context-menu'
import { DateDivider } from '@/components/conversation-list'
import { FilterBar } from '@/components/conversation-list-filter-bar'
import { ListSearchInput } from '@/components/list-search-input'
import { SenderAvatar } from '@/components/sender-avatar'
import { useDeleteMutation } from '@/hooks/use-mail-mutations'
import { useSentMessagesQuery } from '@/hooks/use-sent-messages'
import { extractEmail, extractName } from '@/lib/avatar'
import { dateGroupLabel, formatFullDate } from '@/lib/format'
import { focusedMessageUidAtom, mobileViewAtom, selectedThreadIdAtom } from '@/store/ui'

// rows interleaved with Today / Yesterday / weekday group pills, same
// grouping the inbox list uses.
type SentListItem = { label: string; type: 'divider' } | { msg: WireSentMessage; type: 'row' }

// per-message Sent view: one row per outbound message (not per thread),
// showing the recipient. clicking opens the thread and focuses this exact
// message. Renders the FilterBar so tab navigation stays available (like
// DraftsList — it replaces ConversationList wholesale).
export function SentList() {
  const { data: messages = [], isLoading } = useSentMessagesQuery()
  const setSelectedThreadId = useSetAtom(selectedThreadIdAtom)
  const setFocusedMsgUid = useSetAtom(focusedMessageUidAtom)
  const setMobileView = useSetAtom(mobileViewAtom)
  const [query, setQuery] = useState('')

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return messages
    return messages.filter(
      (m) => m.to.toLowerCase().includes(q) || m.subject.toLowerCase().includes(q)
    )
  }, [messages, query])

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
    if (filtered.length === 0) {
      return <div className="text-fg-muted p-8 text-center text-sm">No matching messages</div>
    }
    return (
      <div className="flex flex-col">
        {/* h-16 + border-l-[3px] matches ROW_BASE in conversation-list so
            every list's rows share one height and avatar column. sent mail
            is read by definition — muted read-row palette. */}
        {groupByDate(filtered).map((item) => {
          if (item.type === 'divider') {
            return <DateDivider key={`d:${item.label}`} label={item.label} />
          }
          return <SentRow key={item.msg.uid} msg={item.msg} onOpen={openMessage} />
        })}
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <ListSearchInput
        label="Search sent"
        onChange={setQuery}
        placeholder="Search sent…"
        value={query}
      />
      <FilterBar />
      <div className="min-h-0 flex-1 overflow-y-auto">{renderBody()}</div>
    </div>
  )
}

// One Sent row + its context menu. Right-click / long-press opens
// Open / Delete — parity with the inbox conversation-list, which
// already uses this pattern. Deleting from Sent uses the same
// useDeleteMutation as the thread-view trash button, so its
// optimistic patch + invalidation are shared, not duplicated.
const SentRow = memo(function SentRow({
  msg,
  onOpen,
}: {
  msg: WireSentMessage
  onOpen: (m: WireSentMessage) => void
}) {
  const ctx = useContextMenu()
  const deleteMutation = useDeleteMutation()

  const items = useMemo<ContextMenuItem[]>(
    () => [
      {
        label: 'Open',
        onClick: () => onOpen(msg),
      },
      {
        danger: true,
        label: 'Delete',
        onClick: () => {
          deleteMutation.mutate(
            { threadId: msg.thread_id },
            {
              onError: (err) =>
                toast.error(err instanceof Error ? err.message : 'Failed to delete'),
              onSuccess: () => toast.success('Deleted'),
            }
          )
        },
      },
    ],
    [msg, onOpen, deleteMutation]
  )

  return (
    <div
      onTouchEnd={ctx.onTouchEnd}
      onTouchMove={ctx.onTouchMove}
      onTouchStart={ctx.onTouchStart}
      role="listitem"
    >
      <button
        className="hover:bg-bg-secondary flex h-16 w-full items-start gap-3 border-l-[3px] border-l-transparent px-4 py-2 text-left transition-colors"
        onClick={() => onOpen(msg)}
        onContextMenu={ctx.open}
        type="button"
      >
        <SenderAvatar className="shrink-0" sender={firstRecipient(msg.to)} size={36} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex items-center justify-between gap-2">
            <span className="text-fg-secondary truncate text-sm font-medium">
              {recipientLabel(msg.to)}
            </span>
            <span className="text-fg-muted text-tiny shrink-0">
              {formatFullDate(msg.internal_date)}
            </span>
          </div>
          <span className="text-fg-muted truncate text-sm">{subjectLabel(msg.subject)}</span>
        </div>
      </button>
      <ContextMenu items={items} onClose={ctx.close} position={ctx.position} />
      <ActionSheet items={items} onClose={ctx.close} open={ctx.actionSheetOpen} />
    </div>
  )
})

// first recipient of a possibly-multi To header — the avatar keys off it.
function firstRecipient(to: string): string {
  const first = to.split(',')[0]?.trim() ?? ''
  return first || to
}

function groupByDate(messages: readonly WireSentMessage[]): SentListItem[] {
  const out: SentListItem[] = []
  let prev = ''
  for (const m of messages) {
    const label = dateGroupLabel(m.internal_date)
    if (label !== prev) {
      out.push({ label, type: 'divider' })
      prev = label
    }
    out.push({ msg: m, type: 'row' })
  }
  return out
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
