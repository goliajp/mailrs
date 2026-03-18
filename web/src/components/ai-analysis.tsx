import { Copyable } from '@/components/copy-button'
import type { ThreadMessage } from '@/lib/types'

type Props = {
  message: ThreadMessage
}

export function AiAnalysisPanel({ message }: Props) {
  if (!message.ai_analyzed) return null

  const hasPeople = message.people?.length > 0
  const hasDates = message.dates?.length > 0
  const hasAmounts = message.amounts?.length > 0
  const hasActions = message.action_items?.length > 0
  const hasDetails = hasPeople || hasDates || hasAmounts || hasActions

  return (
    <div className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-5 py-3">
      {/* summary */}
      {message.summary && (
        <p className="select-text text-sm text-[var(--color-text-secondary)]">
          {message.summary}
        </p>
      )}

      {/* risk reason */}
      {message.risk_score > 0 && message.risk_reason && (
        <p className="mt-1 select-text text-xs text-[var(--color-status-warning)]">
          Risk: {message.risk_reason}
        </p>
      )}

      {hasDetails && (
        <div className="mt-2 flex flex-wrap gap-x-6 gap-y-2">
          {/* people */}
          {hasPeople && (
            <div className="min-w-0">
              <span className="text-xs font-medium uppercase tracking-wide text-[var(--color-text-tertiary)]">
                People
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {message.people.map((p, i) => (
                  <span
                    key={i}
                    className="inline-flex select-text items-center gap-1 rounded bg-[var(--color-brand-subtle)] px-2 py-0.5 text-xs text-[var(--color-brand-primary)]"
                  >
                    {p.name}
                    {p.role && (
                      <span className="text-[var(--color-brand-primary)] opacity-70">
                        ({p.role})
                      </span>
                    )}
                    {p.email && (
                      <>
                        {' '}
                        <Copyable value={p.email}>{p.email}</Copyable>
                      </>
                    )}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* dates */}
          {hasDates && (
            <div className="min-w-0">
              <span className="text-xs font-medium uppercase tracking-wide text-[var(--color-text-tertiary)]">
                Dates
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {message.dates.map((d, i) => (
                  <span
                    key={i}
                    className="inline-flex select-text items-center gap-1 rounded bg-[var(--color-status-success-subtle)] px-2 py-0.5 text-xs text-[var(--color-status-success)]"
                    title={d.context}
                  >
                    {d.text}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* amounts */}
          {hasAmounts && (
            <div className="min-w-0">
              <span className="text-xs font-medium uppercase tracking-wide text-[var(--color-text-tertiary)]">
                Amounts
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {message.amounts.map((a, i) => (
                  <span
                    key={i}
                    className="inline-flex items-center gap-1 rounded bg-[var(--color-status-warning-subtle)] px-2 py-0.5 text-xs text-[var(--color-status-warning)]"
                    title={a.context}
                  >
                    <Copyable value={a.text}>{a.text}</Copyable>
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* action items */}
          {hasActions && (
            <div className="min-w-0">
              <span className="text-xs font-medium uppercase tracking-wide text-[var(--color-text-tertiary)]">
                Action Items
              </span>
              <ul className="mt-0.5 space-y-0.5">
                {message.action_items.map((item, i) => (
                  <li
                    key={i}
                    className="flex select-text items-start gap-1.5 text-xs text-[var(--color-text-secondary)]"
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
