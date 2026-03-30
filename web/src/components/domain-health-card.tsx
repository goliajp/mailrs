import type { CheckResult, CheckStatus, DomainCheckReport } from '@/lib/types'

import { Loader2 } from 'lucide-react'
import { useState } from 'react'

const statusConfig: Record<
  CheckStatus,
  { bg: string; border: string; dot: string; label: string; text: string }
> = {
  fail: {
    bg: 'bg-danger/10',
    border: 'border-danger/10',
    dot: 'bg-danger',
    label: 'Fail',
    text: 'text-danger',
  },
  pass: {
    bg: 'bg-success/10',
    border: 'border-success/10',
    dot: 'bg-success',
    label: 'Pass',
    text: 'text-success',
  },
  skip: {
    bg: 'bg-bg-secondary',
    border: 'border-border',
    dot: 'bg-fg-muted',
    label: 'Skip',
    text: 'text-fg-muted',
  },
  warn: {
    bg: 'bg-warning/10',
    border: 'border-warning/10',
    dot: 'bg-warning',
    label: 'Warn',
    text: 'text-warning',
  },
}

type DomainHealthCardProps = {
  readonly checking: boolean
  readonly onRecheck: () => void
  readonly report: DomainCheckReport
}

export function DomainHealthCard({
  checking,
  onRecheck,
  report,
}: DomainHealthCardProps) {
  const checkedAt = new Date(report.checked_at * 1000)
  const timeStr = checkedAt.toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  })

  return (
    <div className="border-border bg-surface overflow-hidden rounded-lg border shadow-sm">
      {/* header */}
      <div className="border-border flex items-center justify-between border-b px-4 py-3">
        <div>
          <h3 className="text-fg text-sm font-semibold">Health Check</h3>
          <p className="text-fg-secondary text-xs">Last checked at {timeStr}</p>
        </div>
        <button
          className="bg-surface text-fg-secondary hover:bg-bg-secondary inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors disabled:opacity-50"
          disabled={checking}
          onClick={onRecheck}
        >
          {checking ? (
            <>
              <Loader2 className="h-3 w-3 animate-spin" />
              Running...
            </>
          ) : (
            'Re-check'
          )}
        </button>
      </div>

      {/* summary bar */}
      <div className="border-border border-b px-4 py-3">
        <SummaryBar checks={report.checks} />
      </div>

      {/* check results */}
      <div className="space-y-2 p-3">
        {report.checks.map((check) => (
          <CheckResultCard check={check} key={check.name} />
        ))}
      </div>
    </div>
  )
}

function CheckResultCard({ check }: { check: CheckResult }) {
  const [expanded, setExpanded] = useState(false)
  const config = statusConfig[check.status]
  const hasDetails = check.details.length > 0

  return (
    <div
      className={`rounded-lg border ${config.border} ${config.bg} transition-colors`}
    >
      <button
        className={`flex w-full items-center gap-3 px-3 py-2.5 text-left ${hasDetails ? 'cursor-pointer' : 'cursor-default'}`}
        onClick={() => hasDetails && setExpanded(!expanded)}
      >
        <StatusBadge status={check.status} />
        <span className="min-w-0 flex-1">
          <span className="text-fg block text-sm font-medium">
            {check.name}
          </span>
          <span className={`block text-xs ${config.text}`}>
            {check.message}
          </span>
        </span>
        {hasDetails && (
          <svg
            className={`text-fg-muted h-4 w-4 shrink-0 transition-transform ${expanded ? 'rotate-180' : ''}`}
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            viewBox="0 0 24 24"
          >
            <path
              d="M19 9l-7 7-7-7"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        )}
      </button>
      {expanded && hasDetails && (
        <div className="border-t border-inherit px-3 py-2">
          <div className="space-y-1">
            {check.details.map((detail, i) => (
              <pre
                className="bg-bg-secondary text-fg-secondary rounded-md px-2 py-1 font-mono text-xs break-all whitespace-pre-wrap"
                key={i}
              >
                {detail}
              </pre>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

function pctToWidth(pct: number): string {
  if (pct >= 95) return 'w-full'
  if (pct >= 85) return 'w-[85%]'
  if (pct >= 75) return 'w-3/4'
  if (pct >= 65) return 'w-[65%]'
  if (pct >= 55) return 'w-[55%]'
  if (pct >= 45) return 'w-[45%]'
  if (pct >= 35) return 'w-[35%]'
  if (pct >= 25) return 'w-1/4'
  if (pct >= 15) return 'w-[15%]'
  if (pct >= 8) return 'w-[10%]'
  if (pct > 0) return 'w-[5%]'
  return 'w-0'
}

function StatusBadge({ status }: { status: CheckStatus }) {
  const config = statusConfig[status]
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-xs font-medium ${config.bg} ${config.text}`}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${config.dot}`} />
      {config.label}
    </span>
  )
}

function SummaryBar({ checks }: { checks: CheckResult[] }) {
  const counts = checks.reduce(
    (acc, c) => ({ ...acc, [c.status]: (acc[c.status] ?? 0) + 1 }),
    {} as Record<string, number>
  )
  const total = checks.length

  return (
    <div className="flex items-center gap-4">
      <div className="bg-surface flex h-2 flex-1 overflow-hidden rounded-full">
        {(['pass', 'warn', 'fail', 'skip'] as const).map((status) => {
          const count = counts[status] ?? 0
          if (count === 0) return null
          const pct = Math.round((count / total) * 100)
          const colors: Record<CheckStatus, string> = {
            fail: 'bg-danger',
            pass: 'bg-success',
            skip: 'bg-border',
            warn: 'bg-warning',
          }
          return (
            <div
              className={`${colors[status]} ${pctToWidth(pct)} transition-all`}
              key={status}
            />
          )
        })}
      </div>
      <div className="text-fg-secondary flex shrink-0 gap-3 text-xs">
        {counts.pass ? (
          <span className="text-success">{counts.pass} pass</span>
        ) : null}
        {counts.warn ? (
          <span className="text-warning">{counts.warn} warn</span>
        ) : null}
        {counts.fail ? (
          <span className="text-danger">{counts.fail} fail</span>
        ) : null}
        {counts.skip ? (
          <span className="text-fg-secondary">{counts.skip} skip</span>
        ) : null}
      </div>
    </div>
  )
}
