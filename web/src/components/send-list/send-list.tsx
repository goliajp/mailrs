import type { SendRow } from './send-model'
import type { WireSendStatus } from '@/wire/schemas/sends'

import { toast } from '@goliapkg/gds'
import { useAtom, useSetAtom } from 'jotai'
import { memo, useEffect, useMemo, useState } from 'react'

import { DateDivider } from '@/components/conversation-list'
import { FilterBar } from '@/components/conversation-list-filter-bar'
import { ListSearchInput } from '@/components/list-search-input'
import { SenderAvatar } from '@/components/sender-avatar'
import { useResendMutation, useSendsQuery } from '@/hooks/use-sends'
import { useSentMessagesQuery } from '@/hooks/use-sent-messages'
import { extractEmail, extractName } from '@/lib/avatar'
import { dateGroupLabel, formatFullDate } from '@/lib/format'
import { mailRowClass } from '@/lib/list-row-class'
import {
  composeRedraftSourceAtom,
  composingNewAtom,
  focusedMessageUidAtom,
  mobileViewAtom,
  selectedThreadIdAtom,
} from '@/store/ui'
import { wireGetRedraft } from '@/wire/endpoints/sends'

import { FailureDetail } from './failure-detail'
import { filterByStatus, joinSends, needsAttention } from './send-model'
import { StatusBadge } from './status-badge'
import { StatusFilter } from './status-filter'

type Item = { label: string; type: 'divider' } | { row: SendRow; type: 'row' }

/**
 * Send — outbound mail with its delivery state.
 *
 * Two sources, joined on `message_id`: the sent axis for the messages,
 * the Send projection for their status. The projection only covers sends
 * made since it shipped, so most rows have no status and show none — see
 * `send-model.ts` for why that is the honest rendering rather than a
 * default badge.
 *
 * Rows key on `message_id`, never `uid`: `applyOptimisticSent` writes
 * `uid: 0` on every placeholder, so two fresh sends collide on that key
 * and React leaves the stale node behind — two sends rendered three rows
 * on 2026-07-30 (`.claude/rules/frontend/react-key-contract.md`).
 */
export function SendList() {
  const { data: messages, isLoading } = useSentMessagesQuery()
  const { data: sends } = useSendsQuery()
  const [selectedThreadId, setSelectedThreadId] = useAtom(selectedThreadIdAtom)
  const setFocusedMsgUid = useSetAtom(focusedMessageUidAtom)
  const setMobileView = useSetAtom(mobileViewAtom)
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<null | WireSendStatus>(null)
  const [expanded, setExpanded] = useState<null | string>(null)
  const setRedraftSource = useSetAtom(composeRedraftSourceAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const resend = useResendMutation()

  const rows = useMemo(() => joinSends(messages ?? [], sends ?? []), [messages, sends])

  const visible = useMemo(() => {
    const byStatus = filterByStatus(rows, status)
    const q = query.trim().toLowerCase()
    if (!q) return byStatus
    return byStatus.filter(
      (r) => r.to.toLowerCase().includes(q) || r.subject.toLowerCase().includes(q)
    )
  }, [rows, status, query])

  const attention = useMemo(() => rows.filter(needsAttention).length, [rows])

  // Arriving at Send selects its first row.
  //
  // The selection is a global atom and this list only ever wrote it, so
  // switching here from the Inbox left the reading pane showing a thread
  // from the list you had left. Same rule as the conversation list, and it
  // sets the atom directly rather than going through `openMessage`, which
  // also switches the mobile view to the thread — that would drag a phone
  // user into a message they did not open.
  useEffect(() => {
    if (selectedThreadId !== null) return
    const first = visible[0]
    if (!first) return
    setSelectedThreadId(first.threadId)
    setFocusedMsgUid(first.uid)
  }, [visible, selectedThreadId, setSelectedThreadId, setFocusedMsgUid])

  // Leaving clears it, so the list taken to next chooses its own first row
  // rather than inheriting a thread that is not in it.
  useEffect(() => {
    return () => setSelectedThreadId(null)
  }, [setSelectedThreadId])

  const openMessage = (row: SendRow) => {
    setSelectedThreadId(row.threadId)
    // Null when the sweep has not indexed the maildir copy yet. Focusing
    // nothing opens the thread without scrolling to a specific message,
    // which is the same degradation an optimistic row has.
    setFocusedMsgUid(row.uid)
    setMobileView('thread')
  }

  /**
   * Open the composer on a failed send.
   *
   * Fetched here rather than through a query hook because it happens once,
   * on a click, and the composer needs the fields before it mounts. The
   * attachments arrive as descriptions — the bytes stayed on the server
   * and go back out by index.
   */
  const handleRedraft = async (sendId: string) => {
    try {
      const draft = await wireGetRedraft(sendId)
      setRedraftSource({
        attachments: draft.attachments,
        bcc: draft.bcc.join(', '),
        body: draft.body,
        cc: draft.cc.join(', '),
        inReplyTo: draft.in_reply_to ?? null,
        redraftOf: draft.redraft_of,
        subject: draft.subject,
        to: draft.to.join(', '),
      })
      setComposingNew(true)
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Could not open this send for editing')
    }
  }

  const handleResend = (sendId: string) => {
    resend.mutate(sendId, {
      onError: (err) => toast.error(err instanceof Error ? err.message : 'Could not resend'),
      onSuccess: () => toast.success('Sending again'),
    })
  }

  const renderBody = () => {
    if (isLoading) return <div className="text-fg-muted p-4 text-xs">Loading…</div>
    if (rows.length === 0) {
      return <div className="text-fg-muted p-8 text-center text-sm">Nothing sent yet</div>
    }
    if (visible.length === 0) {
      return <div className="text-fg-muted p-8 text-center text-sm">No matching messages</div>
    }
    return (
      <div className="flex flex-col">
        {groupByDate(visible).map((item) => {
          if (item.type === 'divider') {
            return <DateDivider key={`d:${item.label}`} label={item.label} />
          }
          const id = item.row.messageId
          return (
            <SendRowView
              expanded={expanded === id}
              key={id}
              onOpen={openMessage}
              onRedraft={() => void handleRedraft(item.row.send?.send_id ?? '')}
              onResend={handleResend}
              onToggle={() => setExpanded(toggle(expanded, id))}
              resending={resend.isPending}
              row={item.row}
              selected={selectedThreadId === item.row.threadId}
            />
          )
        })}
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <ListSearchInput
        label="Search send"
        onChange={setQuery}
        placeholder="Search send…"
        value={query}
      />
      <FilterBar />
      <StatusFilter attention={attention} onChange={setStatus} value={status} />
      <div className="min-h-0 flex-1 overflow-y-auto">{renderBody()}</div>
    </div>
  )
}

const SendRowView = memo(function SendRowView({
  expanded,
  onOpen,
  onRedraft,
  onResend,
  onToggle,
  resending,
  row,
  selected,
}: {
  expanded: boolean
  onOpen: (row: SendRow) => void
  onRedraft: () => void
  onResend: (sendId: string) => void
  onToggle: () => void
  resending: boolean
  row: SendRow
  selected: boolean
}) {
  const flagged = needsAttention(row)

  return (
    <div role="listitem">
      <button
        className={rowClass(flagged, selected)}
        onClick={() => rowAction(flagged, onToggle, () => onOpen(row))}
        type="button"
      >
        <SenderAvatar className="shrink-0" sender={firstRecipient(row.to)} size={36} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex items-center justify-between gap-2">
            <span className="text-fg-secondary truncate text-sm font-medium">
              {recipientLabel(row.to)}
            </span>
            <span className="text-fg-muted text-tiny shrink-0">{formatFullDate(row.date)}</span>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-fg-muted min-w-0 flex-1 truncate text-sm">
              {subjectLabel(row.subject)}
            </span>
            <StatusBadge status={row.send?.status ?? null} />
          </div>
        </div>
      </button>
      {expanded && row.send && (
        <FailureDetail
          onRedraft={onRedraft}
          onResend={() => onResend(row.send?.send_id ?? '')}
          resending={resending}
          send={row.send}
        />
      )}
    </div>
  )
})

function firstRecipient(to: string): string {
  const first = to.split(',')[0]?.trim() ?? ''
  return first || to
}

function groupByDate(rows: readonly SendRow[]): Item[] {
  const out: Item[] = []
  let prev = ''
  for (const row of rows) {
    const label = dateGroupLabel(row.date)
    if (label !== prev) {
      out.push({ label, type: 'divider' })
      prev = label
    }
    out.push({ row, type: 'row' })
  }
  return out
}

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

/**
 * A flagged row expands; a normal one opens the thread.
 *
 * The click means different things because the rows do: on a delivered
 * send the useful action is reading it, and on a failed one it is finding
 * out why. Making both open the thread would bury the reply text that is
 * the only actionable thing on the screen.
 */
function rowAction(flagged: boolean, onToggle: () => void, onOpen: () => void) {
  if (flagged) {
    onToggle()
    return
  }
  onOpen()
}

function rowClass(flagged: boolean, selected: boolean): string {
  // The same definition the conversation list uses. This had its own copy
  // with no selected state at all, so nothing on the Send list showed which
  // message the reading pane was displaying.
  return mailRowClass({ flagged, selected })
}

function subjectLabel(subject: string): string {
  if (subject.trim()) return subject
  return '(no subject)'
}

function toggle(current: null | string, id: string): null | string {
  if (current === id) return null
  return id
}
