import { Search, X } from 'lucide-react'

// the search header used by the client-filtered list views (Drafts,
// Sent). Matches ConversationList's search styling; controlled so each
// list owns its own filter state (no shared atom → no cross-tab bleed).
export function ListSearchInput({
  label,
  onChange,
  placeholder = 'Search...',
  value,
}: {
  label: string
  onChange: (v: string) => void
  placeholder?: string
  value: string
}) {
  const active = value.trim().length > 0
  return (
    <div className="border-border flex items-center gap-2 border-b px-3 py-2">
      <div className="relative flex-1" role="search">
        <Search
          aria-hidden="true"
          className="text-fg-muted absolute top-1/2 left-2.5 h-4 w-4 -translate-y-1/2"
        />
        <input
          aria-label={label}
          className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:bg-bg w-full rounded-md border py-2 pr-8 pl-9 text-sm transition-colors outline-none"
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          type="text"
          value={value}
        />
        {active && (
          <button
            aria-label="Clear search"
            className="text-fg-muted hover:text-fg-secondary absolute top-1/2 right-2 -translate-y-1/2 rounded p-0.5"
            onClick={() => onChange('')}
            type="button"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
    </div>
  )
}
