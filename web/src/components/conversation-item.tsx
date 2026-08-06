import type { ContextMenuItem } from '@/components/context-menu'
import type { SingleAction } from '@/components/conversation-actions'
import type { ConversationSummary } from '@/lib/types'

import { Check, Mail, Pin, Star } from 'lucide-react'
import { memo, useMemo } from 'react'

import { CategoryBadge, ImportanceBadge } from '@/components/category-badge'
import { ActionSheet, ContextMenu, useContextMenu } from '@/components/context-menu'
import { SenderAvatar } from '@/components/sender-avatar'
import { extractEmail, extractName } from '@/lib/avatar'
import { formatFullDate } from '@/lib/format'
import {
  MAIL_ROW_CONTENT,
  MAIL_ROW_FOCUS,
  MAIL_ROW_FRAME,
  mailRowStateClass,
} from '@/lib/list-row-class'

export const ConversationItem = memo(function ConversationItem({
  batchMode,
  checked,
  convo,
  isJunkView,
  isNpView,
  myEmail,
  onContextAction,
  onSelect,
  onToggleCheck,
  selected,
}: {
  batchMode: boolean
  checked: boolean
  convo: ConversationSummary
  /**
   * v2.4.1 Phase 3 (RFC-B §3.8) — whether this row renders inside the
   * Junk folder view. Drives which of `Mark as junk` /
   * `Mark as not junk` appears in the context menu.
   */
  isJunkView: boolean
  /**
   * v2.9 triage — whether this row renders inside the merged N & P
   * view. Drives the "Move to Inbox" context item.
   */
  isNpView: boolean
  myEmail: string
  onContextAction: (threadId: string, action: SingleAction) => void
  onSelect: (threadId: string) => void
  onToggleCheck: (threadId: string) => void
  selected: boolean
}) {
  // the row's face is the OTHER side of the conversation (Gmail rule:
  // "Thripura, me (7)" — never bare "Me"). After the user replies their
  // own address can bubble to participants[0], which used to flip the
  // row to "Me" and read like a sent mail sitting in the Inbox
  // (2026-07-17). Only a self-only thread falls back to Me.
  const others = convo.participants.filter((p) => extractEmail(p) !== myEmail)
  const displayParticipant = others[0] ?? convo.participants[0] ?? ''
  const isOwn = others.length === 0
  const name = isOwn ? 'Me' : extractName(displayParticipant)
  const extraParticipants = Math.max(0, others.length - 1)
  const hasUnread = convo.unread_count > 0
  const isFlagged = convo.flagged
  const isPinned = convo.pinned
  const isArchived = convo.archived

  const ctx = useContextMenu()

  // Stable references for the memo'd item — without useMemo these rebuild
  // every render, defeating React.memo and forcing the entire list of 50+
  // rows to re-render on every parent update (WebSocket tick, hover on
  // another row when group-hover used to be useState, etc).
  const contextItems = useMemo<ContextMenuItem[]>(
    () => [
      {
        label: hasUnread ? 'Mark as read' : 'Mark as unread',
        onClick: () => onContextAction(convo.thread_id, hasUnread ? 'read' : 'unread'),
      },
      {
        label: isFlagged ? 'Unstar' : 'Star',
        onClick: () => onContextAction(convo.thread_id, isFlagged ? 'unstar' : 'star'),
      },
      {
        label: isPinned ? 'Unpin' : 'Pin',
        onClick: () => onContextAction(convo.thread_id, isPinned ? 'unpin' : 'pin'),
      },
      {
        label: isArchived ? 'Unarchive' : 'Archive',
        onClick: () => onContextAction(convo.thread_id, isArchived ? 'unarchive' : 'archive'),
      },
      {
        label: 'Snooze until tomorrow',
        onClick: () => onContextAction(convo.thread_id, 'snooze'),
      },
      // v2.9 triage — bucket moves, contextual to the current view:
      //   Junk view  → "Mark as not junk" (back to Inbox)
      //   N & P view → "Move to Inbox" + "Mark as junk"
      //   Inbox/else → "Mark as Notification" / "Mark as Promotion" /
      //                "Mark as junk"
      ...(isJunkView
        ? [
            {
              label: 'Mark as not junk',
              onClick: () => onContextAction(convo.thread_id, 'mark-not-junk'),
            },
          ]
        : isNpView
          ? [
              {
                label: 'Move to Inbox',
                onClick: () => onContextAction(convo.thread_id, 'move-to-inbox'),
              },
              {
                label: 'Mark as junk',
                onClick: () => onContextAction(convo.thread_id, 'mark-junk'),
              },
            ]
          : [
              {
                label: 'Mark as Notification',
                onClick: () => onContextAction(convo.thread_id, 'mark-notification'),
              },
              {
                label: 'Mark as Promotion',
                onClick: () => onContextAction(convo.thread_id, 'mark-promotion'),
              },
              {
                label: 'Mark as junk',
                onClick: () => onContextAction(convo.thread_id, 'mark-junk'),
              },
            ]),
      {
        danger: true,
        label: 'Delete',
        onClick: () => onContextAction(convo.thread_id, 'delete'),
      },
    ],
    [
      convo.thread_id,
      hasUnread,
      isFlagged,
      isPinned,
      isArchived,
      isJunkView,
      isNpView,
      onContextAction,
    ]
  )

  const handleClick = () => {
    if (batchMode) {
      onToggleCheck(convo.thread_id)
    } else {
      onSelect(convo.thread_id)
    }
  }

  return (
    <div
      className={`group ${MAIL_ROW_FRAME} ${getRowStateClass({ batchMode, checked, hasUnread, selected })}`}
      onTouchEnd={ctx.onTouchEnd}
      onTouchMove={ctx.onTouchMove}
      onTouchStart={ctx.onTouchStart}
      role="listitem"
    >
      {/* The row's activation, stretched under the content rather than
          wrapped around it. A `<button>` cannot contain the archive and
          star buttons — no interactive content model — and React said so
          on every render. Empty, so its accessible name is the label
          below and nothing else. */}
      <button
        // `aria-current`, not `aria-selected`: selected is only defined on
        // option / tab / row / gridcell and was ignored here. "The one the
        // reading pane is showing" is what current means.
        aria-current={selected && !batchMode}
        aria-label={`${name}: ${convo.subject || '(no subject)'}${hasUnread ? `, ${convo.unread_count} unread` : ''}${isPinned ? ', pinned' : ''}`}
        // h-24 (96px) — HARD-FIXED row height. The previous design let
        // the row collapse when convo.snippet was empty, which mixed
        // two row-heights into the same list and broke the virtualizer's
        // dynamic-size measureElement path (measureElement race +
        // selected-state re-measure + absolute-positioned siblings ⇒
        // intermittent row overlap, see classic-errors.md). With a
        // fixed height the virtualizer never has to re-measure anything,
        // so the overlap bug class is eliminated by construction —
        // no patch, no hack.
        className={`absolute inset-0 z-0 ${MAIL_ROW_FOCUS}`}
        onClick={handleClick}
        onContextMenu={ctx.open}
        type="button"
      />
      {/* Transparent to the pointer so a tap anywhere lands on the button
          above; the action buttons opt back in. */}
      <div className={`pointer-events-none relative z-10 ${MAIL_ROW_CONTENT}`}>
        {batchMode && (
          <div className="mt-0.5 flex shrink-0 items-center">
            <div
              className={`flex h-5 w-5 items-center justify-center rounded border-2 transition-colors ${
                checked ? 'border-accent bg-accent' : 'border-border bg-bg'
              }`}
            >
              {checked && <Check className="h-3 w-3 text-white" />}
            </div>
          </div>
        )}
        <SenderAvatar sender={displayParticipant} size={36} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <span className={getSenderClass({ hasUnread, isOwn })}>
              {name}
              {extraParticipants > 0 && (
                <span className="text-fg-muted"> +{extraParticipants}</span>
              )}
            </span>
            <div className="flex shrink-0 items-center gap-1.5">
              {convo.message_count > 1 && (
                <span
                  className="bg-bg-secondary text-fg-muted md:text-tiny rounded px-1 py-px text-xs tabular-nums"
                  title={`${convo.received_count} received · ${convo.sent_count} sent`}
                >
                  {convo.sent_count > 0 && convo.received_count > 0 ? (
                    <>
                      {convo.received_count}↓ {convo.sent_count}↑
                    </>
                  ) : (
                    convo.message_count
                  )}
                </span>
              )}
              {isPinned && <Pin className="text-accent h-3 w-3" />}
              {/* mobile: always show action buttons; desktop: show on hover
                  (group-hover, no useState — keeps the row out of the
                  re-render path for hover changes). */}
              {!batchMode && (
                <span className="pointer-events-auto flex items-center gap-0.5 md:hidden">
                  <button
                    className="touch-target text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary rounded p-1"
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isArchived ? 'unarchive' : 'archive')
                    }}
                    title={isArchived ? 'Unarchive' : 'Archive'}
                  >
                    <Mail className="h-4 w-4" />
                  </button>
                  <button
                    className={getStarClass({ density: 'mobile', isFlagged })}
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isFlagged ? 'unstar' : 'star')
                    }}
                    title={isFlagged ? 'Unstar' : 'Star'}
                  >
                    <Star className="h-4 w-4" fill={isFlagged ? 'currentColor' : 'none'} />
                  </button>
                </span>
              )}
              {/* desktop: hover actions via group-hover */}
              {!batchMode && (
                <span className="pointer-events-auto hidden items-center gap-0.5 md:group-hover:flex">
                  <button
                    className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary rounded p-0.5"
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isArchived ? 'unarchive' : 'archive')
                    }}
                    title={isArchived ? 'Unarchive' : 'Archive'}
                  >
                    <Mail className="h-3.5 w-3.5" />
                  </button>
                  <button
                    className={getStarClass({ density: 'desktop', isFlagged })}
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isFlagged ? 'unstar' : 'star')
                    }}
                    title={isFlagged ? 'Unstar' : 'Star'}
                  >
                    <Star className="h-3.5 w-3.5" fill={isFlagged ? 'currentColor' : 'none'} />
                  </button>
                </span>
              )}
              {/* full date, always visible (compact rows, 2026-07-17) —
                  matches the Sent view */}
              <span className="text-fg-muted text-tiny shrink-0">
                {formatFullDate(convo.last_date)}
              </span>
            </div>
          </div>
          <div className="flex items-center gap-1.5">
            <p
              className={`min-w-0 flex-1 truncate text-sm ${hasUnread ? 'text-fg font-medium' : 'text-fg-muted'}`}
            >
              {convo.subject || '(no subject)'}
            </p>
            {isFlagged && (
              <Star className="text-warning h-3.5 w-3.5 shrink-0" fill="currentColor" />
            )}
            <span className="shrink-0">
              <ImportanceBadge level={convo.importance_level} />
            </span>
            {convo.category && convo.category !== 'general' && (
              <span className="shrink-0">
                <CategoryBadge category={convo.category} />
              </span>
            )}
            {hasUnread && (
              <span className="bg-accent flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full px-1.5 text-xs font-medium text-white">
                {convo.unread_count}
              </span>
            )}
          </div>
          {/* compact rows: no snippet/preview line (2026-07-17, user) */}
        </div>
      </div>
      <ContextMenu items={contextItems} onClose={ctx.close} position={ctx.position} />
      <ActionSheet items={contextItems} onClose={ctx.close} open={ctx.actionSheetOpen} />
    </div>
  )
})

// unified tab bar
// Spam = AI-derived category filter (categoryFilter='spam', see classify.rs).
// Junk = physical Junk mailbox (mb.name='Junk'), populated by sieve / "mark spam" action.

function getRowStateClass({
  batchMode,
  checked,
  hasUnread,
  selected,
}: {
  batchMode: boolean
  checked: boolean
  hasUnread: boolean
  selected: boolean
}): string {
  return mailRowStateClass({ batchMode, checked, muted: !hasUnread, selected })
}

function getSenderClass({ hasUnread, isOwn }: { hasUnread: boolean; isOwn: boolean }): string {
  // hasUnread wins over isOwn — same effective cascade as the previous
  // double-ternary (`text-accent text-fg ...` resolves to the last token).
  const color = hasUnread ? 'text-fg font-semibold' : isOwn ? 'text-accent' : 'text-fg-secondary'
  return `truncate text-sm ${color}`
}

function getStarClass({
  density,
  isFlagged,
}: {
  density: 'desktop' | 'mobile'
  isFlagged: boolean
}): string {
  const base =
    density === 'mobile' ? 'touch-target rounded p-1' : 'hover:bg-bg-secondary rounded p-0.5'
  const color = isFlagged ? 'text-warning' : 'text-fg-muted hover:text-fg-secondary'
  return `${base} ${color}`
}
