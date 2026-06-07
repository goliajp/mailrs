import type { ReplyMode } from './types'

import { MODE_LABELS } from './types'

type ModeToggleProps = {
  mode: ReplyMode
  onChange: (mode: ReplyMode) => void
  replyAllRecipients: string
  replyRecipients: string
}

export function ModeToggle({
  mode,
  onChange,
  replyAllRecipients,
  replyRecipients,
}: ModeToggleProps) {
  const toLabel = mode === 'reply' ? replyRecipients : replyAllRecipients

  return (
    <div className="border-border flex shrink-0 items-center gap-1 border-b px-4 py-2 select-none">
      {(Object.keys(MODE_LABELS) as ReplyMode[]).map((m) => (
        <button
          aria-pressed={mode === m}
          className={`focus-visible:ring-accent cursor-pointer rounded px-2.5 py-2 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none md:py-1 ${
            mode === m
              ? 'bg-accent/10 text-accent'
              : 'text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary'
          }`}
          key={m}
          onClick={() => onChange(m)}
          type="button"
        >
          {MODE_LABELS[m]}
        </button>
      ))}
      {mode !== 'forward' && (
        <span className="text-fg-muted ml-auto min-w-0 truncate text-xs" title={toLabel}>
          to {toLabel}
        </span>
      )}
    </div>
  )
}
