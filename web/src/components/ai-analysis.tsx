import type { ThreadMessage } from '@/lib/types'

import { ChevronDown, ChevronRight } from 'lucide-react'
import { useState } from 'react'

import { Copyable } from '@/components/copy-button'

type Props = {
  message: ThreadMessage
}

export function AiAnalysisPanel({ message }: Props) {
  const [expanded, setExpanded] = useState(false)

  if (!message.ai_analyzed) return null

  const people = safePeople(message.people)
  const dates = safeDates(message.dates)
  const amounts = safeAmounts(message.amounts)
  const actions = Array.isArray(message.action_items)
    ? message.action_items.filter((s) => typeof s === 'string' && s.trim())
    : []
  const hasDeadline = !!message.action_deadline
  const hasDetails =
    people.length > 0 ||
    dates.length > 0 ||
    amounts.length > 0 ||
    actions.length > 0

  return (
    <div className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-5 py-2">
      {/* summary — always visible, clickable to expand */}
      <button
        className="flex w-full items-start gap-1.5 text-left"
        onClick={() => setExpanded((v) => !v)}
      >
        {hasDetails ? (
          expanded ? (
            <ChevronDown className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--color-text-tertiary)]" />
          ) : (
            <ChevronRight className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--color-text-tertiary)]" />
          )
        ) : null}
        <div className="min-w-0 flex-1">
          {message.summary ? (
            <p className="text-sm text-[var(--color-text-secondary)] select-text">
              {message.summary}
            </p>
          ) : hasDetails ? (
            <p className="text-xs text-[var(--color-text-tertiary)]">
              AI analysis details
            </p>
          ) : null}

          {/* deadline */}
          {hasDeadline && (
            <p className="mt-1 text-xs font-medium text-[var(--color-status-danger)] select-text">
              Deadline: {message.action_deadline}
            </p>
          )}

          {/* risk reason */}
          {message.risk_score > 0 && message.risk_reason && (
            <p className="mt-1 text-xs text-[var(--color-status-warning)] select-text">
              Risk: {message.risk_reason}
            </p>
          )}
        </div>
      </button>

      {expanded && hasDetails && (
        <div className="mt-2 flex flex-wrap gap-x-6 gap-y-2">
          {people.length > 0 && (
            <div className="min-w-0">
              <span className="text-xs font-medium tracking-wide text-[var(--color-text-tertiary)] uppercase">
                People
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {people.map((p, i) => (
                  <span
                    className="inline-flex max-w-full items-center gap-1 truncate rounded bg-[var(--color-brand-subtle)] px-2 py-0.5 text-xs text-[var(--color-brand-primary)] select-text"
                    key={i}
                  >
                    {p.label}
                    {p.role && <span className="opacity-70">({p.role})</span>}
                    {p.email && p.email !== p.label && (
                      <Copyable value={p.email}>{p.email}</Copyable>
                    )}
                  </span>
                ))}
              </div>
            </div>
          )}

          {dates.length > 0 && (
            <div className="min-w-0">
              <span className="text-xs font-medium tracking-wide text-[var(--color-text-tertiary)] uppercase">
                Dates
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {dates.map((d, i) => (
                  <span
                    className="inline-flex items-center rounded bg-[var(--color-status-success-subtle)] px-2 py-0.5 text-xs text-[var(--color-status-success)] select-text"
                    key={i}
                    title={d.context}
                  >
                    {d.text}
                  </span>
                ))}
              </div>
            </div>
          )}

          {amounts.length > 0 && (
            <div className="min-w-0">
              <span className="text-xs font-medium tracking-wide text-[var(--color-text-tertiary)] uppercase">
                Amounts
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {amounts.map((a, i) => (
                  <span
                    className="inline-flex items-center rounded bg-[var(--color-status-warning-subtle)] px-2 py-0.5 text-xs text-[var(--color-status-warning)] select-text"
                    key={i}
                    title={a.context}
                  >
                    <Copyable value={a.text}>{a.text}</Copyable>
                  </span>
                ))}
              </div>
            </div>
          )}

          {actions.length > 0 && (
            <div className="min-w-0">
              <span className="text-xs font-medium tracking-wide text-[var(--color-text-tertiary)] uppercase">
                Action Items
              </span>
              <ul className="mt-0.5 space-y-0.5">
                {actions.map((item, i) => (
                  <li
                    className="flex items-start gap-1.5 text-xs break-words text-[var(--color-text-secondary)] select-text"
                    key={i}
                  >
                    <span className="mt-0.5 h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--color-brand-primary)]" />
                    {item}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function safeAmounts(raw: any[]): { context?: string; text: string }[] {
  if (!Array.isArray(raw)) return []
  const results: { context?: string; text: string }[] = []
  for (const a of raw) {
    if (typeof a === 'string' && a.trim()) {
      results.push({ text: a })
      continue
    }
    if (a && typeof a === 'object') {
      const text =
        a.text || (a.value != null ? `${a.currency ?? ''}${a.value}` : '')
      if (text) results.push({ context: a.context || undefined, text })
    }
  }
  return results
}

function safeDates(raw: any[]): { context?: string; text: string }[] {
  if (!Array.isArray(raw)) return []
  const results: { context?: string; text: string }[] = []
  for (const d of raw) {
    if (typeof d === 'string' && d.trim()) {
      results.push({ text: d })
      continue
    }
    if (d && typeof d === 'object') {
      const text = d.text || d.iso_date || ''
      if (text) results.push({ context: d.context || undefined, text })
    }
  }
  return results
}

// safely extract display text from potentially malformed AI data

function safePeople(
  raw: any[]
): { email?: string; label: string; role?: string }[] {
  if (!Array.isArray(raw)) return []
  const results: { email?: string; label: string; role?: string }[] = []
  for (const p of raw) {
    if (typeof p === 'string' && p.trim()) {
      results.push({ label: p })
      continue
    }
    if (p && typeof p === 'object') {
      const label = p.name || p.email || ''
      if (label)
        results.push({
          email: p.email || undefined,
          label,
          role: p.role || undefined,
        })
    }
  }
  return results
}
