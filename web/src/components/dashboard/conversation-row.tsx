import type { ConversationSummary } from '@/lib/types'

import { useAtomValue } from 'jotai'
import { Pin, Star } from 'lucide-react'

import { CategoryBadge } from '@/components/category-badge'
import { SenderAvatar } from '@/components/sender-avatar'
import { extractEmail } from '@/lib/avatar'
import { cn } from '@/lib/cn'
import { authAtom } from '@/store/auth'

import { extractName, formatRelative } from './_shared'

export function ConversationRow({
  conversation: c,
  onClick,
}: {
  conversation: ConversationSummary
  onClick: () => void
}) {
  const auth = useAtomValue(authAtom)
  const myEmail = auth?.address ?? ''
  // show the OTHER side of the conversation, never bare "Me" (same
  // Gmail rule as the main list rows)
  const sender = c.participants.find((p) => extractEmail(p) !== myEmail) ?? c.participants[0] ?? ''
  const isUnread = c.unread_count > 0
  return (
    <button
      className="hover:bg-bg-secondary flex w-full items-center gap-3 rounded-md px-2 py-2 text-left transition-colors"
      onClick={onClick}
    >
      <SenderAvatar sender={sender} size={32} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span
            className={cn('text-fg truncate text-sm', isUnread ? 'font-semibold' : 'font-medium')}
          >
            {extractName(sender)}
          </span>
          <CategoryBadge category={c.category} />
          {c.flagged && (
            <Star aria-label="Starred" className="h-3 w-3 shrink-0 fill-amber-500 text-amber-500" />
          )}
          {c.pinned && <Pin aria-label="Pinned" className="text-fg-muted h-3 w-3 shrink-0" />}
        </div>
        <p
          className={cn('truncate text-xs', isUnread ? 'text-fg font-medium' : 'text-fg-secondary')}
        >
          {c.subject || '(no subject)'}
        </p>
        {c.snippet && <p className="text-fg-muted mt-0.5 truncate text-xs">{c.snippet}</p>}
      </div>
      <div className="flex shrink-0 flex-col items-end gap-1">
        <span className="text-fg-muted text-xs tabular-nums">{formatRelative(c.last_date)}</span>
        {isUnread && (
          <span className="bg-accent text-tiny flex h-4.5 min-w-4.5 items-center justify-center rounded-full px-1 font-medium text-white">
            {c.unread_count}
          </span>
        )}
      </div>
    </button>
  )
}
