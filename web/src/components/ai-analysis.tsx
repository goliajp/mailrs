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
    <div className="border-b border-zinc-200 bg-zinc-50 px-5 py-3 dark:border-zinc-800 dark:bg-zinc-900/50">
      {/* summary */}
      {message.summary && (
        <p className="text-sm text-zinc-700 dark:text-zinc-300">
          {message.summary}
        </p>
      )}

      {/* risk reason */}
      {message.risk_score > 0 && message.risk_reason && (
        <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
          Risk: {message.risk_reason}
        </p>
      )}

      {hasDetails && (
        <div className="mt-2 flex flex-wrap gap-x-6 gap-y-2">
          {/* people */}
          {hasPeople && (
            <div className="min-w-0">
              <span className="text-xs font-medium uppercase tracking-wide text-zinc-400">
                People
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {message.people.map((p, i) => (
                  <span
                    key={i}
                    className="inline-flex items-center gap-1 rounded-full bg-blue-50 px-2 py-0.5 text-xs text-blue-700 dark:bg-blue-950/50 dark:text-blue-300"
                  >
                    {p.name}
                    {p.role && (
                      <span className="text-blue-400 dark:text-blue-500">
                        ({p.role})
                      </span>
                    )}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* dates */}
          {hasDates && (
            <div className="min-w-0">
              <span className="text-xs font-medium uppercase tracking-wide text-zinc-400">
                Dates
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {message.dates.map((d, i) => (
                  <span
                    key={i}
                    className="inline-flex items-center gap-1 rounded-full bg-green-50 px-2 py-0.5 text-xs text-green-700 dark:bg-green-950/50 dark:text-green-300"
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
              <span className="text-xs font-medium uppercase tracking-wide text-zinc-400">
                Amounts
              </span>
              <div className="mt-0.5 flex flex-wrap gap-1">
                {message.amounts.map((a, i) => (
                  <span
                    key={i}
                    className="inline-flex items-center gap-1 rounded-full bg-amber-50 px-2 py-0.5 text-xs text-amber-700 dark:bg-amber-950/50 dark:text-amber-300"
                    title={a.context}
                  >
                    {a.text}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* action items */}
          {hasActions && (
            <div className="min-w-0">
              <span className="text-xs font-medium uppercase tracking-wide text-zinc-400">
                Action Items
              </span>
              <ul className="mt-0.5 space-y-0.5">
                {message.action_items.map((item, i) => (
                  <li
                    key={i}
                    className="flex items-start gap-1.5 text-xs text-zinc-600 dark:text-zinc-400"
                  >
                    <span className="mt-0.5 h-1.5 w-1.5 shrink-0 rounded-full bg-violet-400" />
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
