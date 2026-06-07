type SuggestionsRowProps = {
  onApply: (suggestion: string) => void
  onDismiss: () => void
  suggestions: string[]
}

export function SuggestionsRow({ onApply, onDismiss, suggestions }: SuggestionsRowProps) {
  if (suggestions.length === 0) return null
  return (
    <div className="border-border flex shrink-0 flex-wrap gap-1.5 border-b px-4 py-2">
      {suggestions.map((s, i) => (
        <button
          className="border-border bg-accent/10 text-accent hover:bg-bg-secondary max-w-xs truncate rounded-full border px-2.5 py-0.5 text-xs transition-colors"
          key={i}
          onClick={() => onApply(s)}
          title={s}
          type="button"
        >
          {s}
        </button>
      ))}
      <button
        className="text-fg-muted hover:text-fg-secondary rounded-full px-2 py-0.5 text-xs transition-colors"
        onClick={onDismiss}
        type="button"
      >
        Dismiss
      </button>
    </div>
  )
}
