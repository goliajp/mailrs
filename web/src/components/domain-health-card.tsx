import { Loader2 } from 'lucide-react'
import { useState } from 'react'

import type { CheckResult, CheckStatus, DomainCheckReport } from '@/lib/types'

const statusConfig: Record<CheckStatus, { bg: string; border: string; text: string; dot: string; label: string }> = {
  pass: {
    bg: 'bg-[var(--color-status-success-subtle)]',
    border: 'border-green-200 dark:border-green-800/50',
    text: 'text-[var(--color-status-success)]',
    dot: 'bg-green-500',
    label: 'Pass',
  },
  warn: {
    bg: 'bg-yellow-50 dark:bg-yellow-950/30',
    border: 'border-yellow-200 dark:border-yellow-800/50',
    text: 'text-yellow-700 dark:text-yellow-400',
    dot: 'bg-yellow-500',
    label: 'Warn',
  },
  fail: {
    bg: 'bg-[var(--color-status-danger-subtle)]',
    border: 'border-red-200 dark:border-red-800/50',
    text: 'text-[var(--color-status-danger)]',
    dot: 'bg-red-500',
    label: 'Fail',
  },
  skip: {
    bg: 'bg-[var(--color-bg-sunken)]',
    border: 'border-[var(--color-border-default)]',
    text: 'text-[var(--color-text-tertiary)]',
    dot: 'bg-zinc-400',
    label: 'Skip',
  },
}

function StatusBadge({ status }: { status: CheckStatus }) {
  const config = statusConfig[status]
  return (
    <span className={`inline-flex items-center gap-1.5 rounded px-2 py-0.5 text-xs font-medium ${config.bg} ${config.text}`}>
      <span className={`h-1.5 w-1.5 rounded-full ${config.dot}`} />
      {config.label}
    </span>
  )
}

function CheckResultCard({ check }: { check: CheckResult }) {
  const [expanded, setExpanded] = useState(false)
  const config = statusConfig[check.status]
  const hasDetails = check.details.length > 0

  return (
    <div className={`rounded border ${config.border} ${config.bg} transition-colors`}>
      <button
        onClick={() => hasDetails && setExpanded(!expanded)}
        className={`flex w-full items-center gap-3 px-3 py-2.5 text-left ${hasDetails ? 'cursor-pointer' : 'cursor-default'}`}
      >
        <StatusBadge status={check.status} />
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-medium text-[var(--color-text-primary)]">
            {check.name}
          </span>
          <span className={`block text-xs ${config.text}`}>
            {check.message}
          </span>
        </span>
        {hasDetails && (
          <svg
            className={`h-4 w-4 shrink-0 text-zinc-400 transition-transform ${expanded ? 'rotate-180' : ''}`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            strokeWidth={2}
          >
            <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
          </svg>
        )}
      </button>
      {expanded && hasDetails && (
        <div className="border-t border-inherit px-3 py-2">
          <div className="space-y-1">
            {check.details.map((detail, i) => (
              <pre
                key={i}
                className="whitespace-pre-wrap break-all rounded bg-white/60 px-2 py-1 font-mono text-xs text-[var(--color-text-secondary)] dark:bg-black/20"
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

function SummaryBar({ checks }: { checks: CheckResult[] }) {
  const counts = checks.reduce(
    (acc, c) => ({ ...acc, [c.status]: (acc[c.status] ?? 0) + 1 }),
    {} as Record<string, number>,
  )
  const total = checks.length

  return (
    <div className="flex items-center gap-4">
      <div className="flex h-2 flex-1 overflow-hidden rounded-full bg-[var(--color-bg-raised)]">
        {(['pass', 'warn', 'fail', 'skip'] as const).map((status) => {
          const count = counts[status] ?? 0
          if (count === 0) return null
          const width = (count / total) * 100
          const colors: Record<CheckStatus, string> = {
            pass: 'bg-green-500',
            warn: 'bg-yellow-500',
            fail: 'bg-red-500',
            skip: 'bg-[var(--color-border-default)]',
          }
          return (
            <div
              key={status}
              className={`${colors[status]} transition-all`}
              style={{ width: `${width}%` }}
            />
          )
        })}
      </div>
      <div className="flex shrink-0 gap-3 text-xs text-zinc-500">
        {counts.pass ? <span className="text-[var(--color-status-success)]">{counts.pass} pass</span> : null}
        {counts.warn ? <span className="text-yellow-600 dark:text-yellow-400">{counts.warn} warn</span> : null}
        {counts.fail ? <span className="text-[var(--color-status-danger)]">{counts.fail} fail</span> : null}
        {counts.skip ? <span className="text-zinc-400">{counts.skip} skip</span> : null}
      </div>
    </div>
  )
}

type DomainHealthCardProps = {
  readonly report: DomainCheckReport
  readonly checking: boolean
  readonly onRecheck: () => void
}

export function DomainHealthCard({ report, checking, onRecheck }: DomainHealthCardProps) {
  const checkedAt = new Date(report.checked_at * 1000)
  const timeStr = checkedAt.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

  return (
    <div className="overflow-hidden rounded border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] shadow-sm">
      {/* header */}
      <div className="flex items-center justify-between border-b border-[var(--color-border-default)] px-4 py-3">
        <div>
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
            Health Check
          </h3>
          <p className="text-xs text-zinc-500">
            Last checked at {timeStr}
          </p>
        </div>
        <button
          onClick={onRecheck}
          disabled={checking}
          className="inline-flex items-center gap-1.5 rounded-md bg-[var(--color-bg-raised)] px-2.5 py-1.5 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
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
      <div className="border-b border-[var(--color-border-default)] px-4 py-3">
        <SummaryBar checks={report.checks} />
      </div>

      {/* check results */}
      <div className="space-y-2 p-3">
        {report.checks.map((check) => (
          <CheckResultCard key={check.name} check={check} />
        ))}
      </div>
    </div>
  )
}
