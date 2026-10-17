import type { MockDirection, MockMessageMeta } from '@/wire/schemas/mock'

import { cn } from '@/lib/cn'

import { formatReceived, subjectLabel } from './format'

export function DirectionBadge({ direction }: { direction: MockDirection }) {
  const { className, label, title } = directionLook(direction)
  return (
    <span
      className={cn('text-tiny shrink-0 rounded px-1.5 py-0.5 font-semibold uppercase', className)}
      title={title}
    >
      {label}
    </span>
  )
}

/** Newest first, as the server returns them. */
export function MessageList({
  dropped,
  messages,
  onSelect,
  selected,
}: {
  dropped: number
  messages: readonly MockMessageMeta[]
  onSelect: (id: string) => void
  selected: null | string
}) {
  return (
    <div className="space-y-1">
      {dropped > 0 && (
        <p className="text-warning text-xs">
          {dropped} older messages were dropped to stay under the namespace cap
        </p>
      )}
      {messages.length === 0 && (
        <p className="text-fg-muted py-8 text-center text-sm">
          No mail yet. This list refreshes every 5 seconds
        </p>
      )}
      <ul aria-label="Messages" className="space-y-1">
        {messages.map((m) => (
          <li key={m.id}>
            <button
              aria-current={m.id === selected || undefined}
              className={cn(
                'w-full rounded-md border px-3 py-2 text-left transition-colors',
                m.id === selected && 'border-accent bg-accent/5',
                m.id !== selected && 'border-border hover:bg-bg-secondary'
              )}
              onClick={() => onSelect(m.id)}
              type="button"
            >
              <span className="flex items-center gap-2">
                <DirectionBadge direction={m.direction} />
                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                  {subjectLabel(m.subject)}
                </span>
                <span className="text-fg-muted shrink-0 text-xs">
                  {formatReceived(m.received_at)}
                </span>
              </span>
              <span className="text-fg-muted mt-0.5 block truncate text-xs">
                {m.from} → {m.rcpt}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}

function directionLook(direction: MockDirection): {
  className: string
  label: string
  title: string
} {
  switch (direction) {
    case 'captured-out':
      return {
        className: 'bg-warning/15 text-warning',
        label: 'captured-out',
        title: 'Sent by this sandbox and kept here instead of being delivered',
      }
    case 'in':
      return {
        className: 'bg-accent/10 text-accent',
        label: 'in',
        title: 'Received at a mock address',
      }
  }
}
