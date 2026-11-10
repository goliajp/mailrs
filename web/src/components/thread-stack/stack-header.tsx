import type { ActionId } from '@/lib/actions'

import { ChevronDown, ChevronUp, Star } from 'lucide-react'

import { HdrBtn } from '@/components/thread-view-bubble'
import { useT } from '@/i18n'
import { ACTIONS, toggle } from '@/lib/actions'

import { PushMenu } from './push-menu'

export type ConversationControls = {
  goToNext: () => void
  goToPrev: () => void
  hasNext: boolean
  hasPrev: boolean
  isArchived: boolean
  isFlagged: boolean
  isRead: boolean
  isTrashed: boolean
  onArchive: () => void
  onMarkRead: () => void
  onMarkUnread: () => void
  onStar: () => void
  onTrash: () => void
  onUnstar: () => void
  onUntrash: () => void
}

/** The conversation's subject and what can be done to all of it. */
export function StackHeader({
  controls: c,
  count,
  subject,
  threadId,
}: {
  controls: ConversationControls
  count: number
  subject: string
  threadId: string
}) {
  const t = useT()
  return (
    <div className="border-border h-pane-header flex shrink-0 items-center gap-2 border-b px-3 select-none">
      <div className="flex min-w-0 flex-1 items-center gap-2">
        <h2 className="text-fg truncate text-sm font-semibold select-text">
          {subject || t('mobile.noSubject')}
        </h2>
        {count > 1 && <span className="text-fg-muted shrink-0 text-xs">{count}</span>}
      </div>
      <div className="flex shrink items-center gap-0.5 lg:gap-1">
        {/* Hidden below `lg`, where the pane can be narrow and the subject
            needs the room; `j`/`k` and the list itself do the same. */}
        <HdrBtn
          className="hidden lg:inline-flex"
          disabled={!c.hasPrev}
          onClick={c.goToPrev}
          title={t('read.previous')}
        >
          <ChevronUp className="h-4 w-4" />
        </HdrBtn>
        <HdrBtn
          className="hidden lg:inline-flex"
          disabled={!c.hasNext}
          onClick={c.goToNext}
          title={t('read.next')}
        >
          <ChevronDown className="h-4 w-4" />
        </HdrBtn>
        <ActionBtn id={toggle(c.isArchived, 'unarchive', 'archive')} onClick={c.onArchive} />
        <ActionBtn
          id={toggle(c.isRead, 'unread', 'read')}
          onClick={readHandler(c.isRead, c.onMarkUnread, c.onMarkRead)}
        />
        <HdrBtn
          className={starClass(c.isFlagged)}
          onClick={readHandler(c.isFlagged, c.onUnstar, c.onStar)}
          title={t(ACTIONS[toggle(c.isFlagged, 'unstar', 'star')].label)}
        >
          <Star className="h-4 w-4" fill={starFill(c.isFlagged)} />
        </HdrBtn>
        <PushMenu threadId={threadId} />
        {c.isTrashed && <ActionBtn id="untrash" onClick={c.onUntrash} />}
        <ActionBtn
          className="hover:text-danger"
          id={toggle(c.isTrashed, 'delete-forever', 'trash')}
          onClick={c.onTrash}
        />
      </div>
    </div>
  )
}

function ActionBtn({
  className,
  id,
  onClick,
}: {
  className?: string
  id: ActionId
  onClick: () => void
}) {
  const t = useT()
  const spec = ACTIONS[id]
  const Icon = spec.icon
  return (
    <HdrBtn className={className} onClick={onClick} title={t(spec.label)}>
      <Icon className="h-4 w-4" />
    </HdrBtn>
  )
}

function readHandler(on: boolean, whenOn: () => void, whenOff: () => void): () => void {
  if (on) return whenOn
  return whenOff
}

function starClass(flagged: boolean): string | undefined {
  if (flagged) return 'text-warning hover:text-warning'
  return undefined
}

function starFill(flagged: boolean): string {
  if (flagged) return 'currentColor'
  return 'none'
}
