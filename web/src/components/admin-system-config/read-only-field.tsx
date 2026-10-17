import type { ReactNode } from 'react'

/** A setting nothing reads while it runs: shown with where it is set, never as an input. */
export function ReadOnlyField({
  badge,
  description,
  envVar,
  label,
  note,
  reset,
  value,
}: {
  badge: ReactNode
  description: string
  envVar?: string
  label: string
  note?: null | string
  reset?: ReactNode
  value: null | string
}) {
  return (
    <div className="border-border border-b px-4 py-4 last:border-0">
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <code className="text-fg text-sm font-medium">{label}</code>
        {badge}
        <span className="text-fg-muted text-xs">Read-only</span>
      </div>
      {description && <p className="text-fg-secondary mb-2 text-xs">{description}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <code
          aria-label={label}
          className="bg-bg-secondary rounded-md px-3 py-1.5 text-sm"
          data-testid={`readonly-${label}`}
        >
          {displayValue(value)}
        </code>
        {envVar && <code className="text-fg-muted text-xs">{envVar}</code>}
        {reset}
      </div>
      {note && <p className="text-fg-muted mt-2 text-xs">{note}</p>}
    </div>
  )
}

// unknown is not empty: a value webapi cannot see renders as a dash
function displayValue(value: null | string): string {
  if (value === null || value === '') return '—'
  return value
}
