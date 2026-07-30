import type { WireSendStatus } from '@/wire/schemas/sends'

import { AlertTriangle, Check, Clock, Loader2, X } from 'lucide-react'

import { statusLabel } from './send-model'

const BASE = 'inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-mini whitespace-nowrap'

/**
 * A send's delivery state.
 *
 * Renders nothing when there is no state to render. Every send made
 * before the projection shipped has no record, and an "unknown" pill on
 * hundreds of historical rows would be noise claiming to be information —
 * absence of a record is shown as absence of a badge
 * (`.claude/rules/common/coding-style.md` → Null vs Zero).
 */
export function StatusBadge({ status }: { status: null | WireSendStatus }) {
  if (!status) return null
  return (
    <span className={`${BASE} ${toneClass(status)}`}>
      <StatusIcon status={status} />
      {statusLabel(status)}
    </span>
  )
}

function StatusIcon({ status }: { status: WireSendStatus }) {
  const cls = 'h-3 w-3'
  switch (status) {
    case 'delivered':
      return <Check aria-hidden className={cls} />
    case 'failed':
      return <X aria-hidden className={cls} />
    case 'partial':
      return <AlertTriangle aria-hidden className={cls} />
    case 'scheduled':
      return <Clock aria-hidden className={cls} />
    case 'sending':
      // Spinning, because this is the one state expected to change on its
      // own — a still icon reads as stuck.
      return <Loader2 aria-hidden className={`${cls} animate-spin`} />
  }
}

function toneClass(status: WireSendStatus): string {
  switch (status) {
    case 'delivered':
      return 'bg-success/10 text-success'
    case 'failed':
      return 'bg-danger/10 text-danger'
    case 'partial':
      return 'bg-warning/10 text-warning'
    case 'scheduled':
      return 'bg-bg-secondary text-fg-secondary'
    case 'sending':
      return 'bg-info/10 text-info'
  }
}
