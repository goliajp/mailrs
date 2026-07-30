import type { WireSendStatus } from '@/wire/schemas/sends'

import { statusLabel } from './send-model'

const OPTIONS: WireSendStatus[] = ['sending', 'failed', 'partial', 'delivered', 'scheduled']

/**
 * Status filter for the Send list, plus a count of the sends that need
 * attention.
 *
 * The count is shown only when it is non-zero. A permanent "0 need
 * attention" is a line of chrome that never says anything, and a badge
 * the eye learns to skip is worse than no badge when it finally has a
 * number in it.
 */
export function StatusFilter({
  attention,
  onChange,
  value,
}: {
  attention: number
  onChange: (status: null | WireSendStatus) => void
  value: null | WireSendStatus
}) {
  return (
    <div className="border-border/60 flex items-center gap-1 overflow-x-auto border-b px-3 py-1.5">
      <button className={chipClass(value === null)} onClick={() => onChange(null)} type="button">
        All
      </button>
      {OPTIONS.map((status) => (
        <button
          className={chipClass(value === status)}
          key={status}
          onClick={() => onChange(pick(value, status))}
          type="button"
        >
          {statusLabel(status)}
        </button>
      ))}
      {attention > 0 && (
        <span className="bg-danger/10 text-danger text-mini ml-auto shrink-0 rounded px-1.5 py-0.5">
          {attentionLabel(attention)}
        </span>
      )}
    </div>
  )
}

function attentionLabel(n: number): string {
  if (n === 1) return '1 needs attention'
  return `${n} need attention`
}

function chipClass(active: boolean): string {
  const base = 'text-mini shrink-0 rounded-md px-2 py-0.5 transition-colors whitespace-nowrap'
  if (active) return `${base} bg-fg text-bg`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}

/** Clicking the active chip clears the filter rather than re-applying it. */
function pick(current: null | WireSendStatus, clicked: WireSendStatus): null | WireSendStatus {
  if (current === clicked) return null
  return clicked
}
