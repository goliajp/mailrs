import type { QueueRow, QueueStatus } from './queue-row'

import { ALL_STATUSES, statusStyles } from './queue-row'

const filterBaseStyle = 'rounded-md px-3 py-1 text-xs font-medium transition-colors cursor-pointer'
const filterActiveStyle = 'ring-2 ring-offset-1 ring-border ring-offset-bg'
const filterAllStyle = 'bg-border text-fg-secondary'

const pagerButton =
  'border-border hover:bg-bg-secondary rounded-md border px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40'

export function QueuePager({
  onPage,
  page,
  pageSize,
  total,
  totalPages,
}: {
  onPage: (page: number) => void
  page: number
  pageSize: number
  total: number
  totalPages: number
}) {
  if (total <= pageSize) return null
  return (
    <div className="mt-4 flex items-center justify-between text-sm">
      <span className="text-fg-secondary tabular-nums">
        {total} jobs &middot; Page {page} / {totalPages}
      </span>
      <div className="flex gap-2">
        <button
          className={pagerButton}
          disabled={page <= 1}
          onClick={() => onPage(page - 1)}
          type="button"
        >
          Previous
        </button>
        <button
          className={pagerButton}
          disabled={page >= totalPages}
          onClick={() => onPage(page + 1)}
          type="button"
        >
          Next
        </button>
      </div>
    </div>
  )
}

export function QueueStatusFilter({
  onChange,
  queue,
  value,
}: {
  onChange: (next: null | QueueStatus) => void
  queue: readonly QueueRow[]
  value: null | QueueStatus
}) {
  return (
    <div className="mb-4 flex flex-wrap gap-2">
      <button
        aria-pressed={value === null}
        className={`${filterBaseStyle} ${filterAllStyle} ${activeIf(value === null)}`}
        onClick={() => onChange(null)}
        type="button"
      >
        All ({queue.length})
      </button>
      {ALL_STATUSES.map((status) => (
        <button
          aria-pressed={value === status}
          className={`${filterBaseStyle} ${statusStyles[status]} ${activeIf(value === status)}`}
          key={status}
          onClick={() => onChange(toggled(value, status))}
          type="button"
        >
          <span className="capitalize">{status}</span> ({countOf(queue, status)})
        </button>
      ))}
    </div>
  )
}

function activeIf(active: boolean): string {
  if (active) return filterActiveStyle
  return ''
}

function countOf(queue: readonly QueueRow[], status: QueueStatus): number {
  return queue.filter((q) => q.status === status).length
}

function toggled(current: null | QueueStatus, status: QueueStatus): null | QueueStatus {
  if (current === status) return null
  return status
}
