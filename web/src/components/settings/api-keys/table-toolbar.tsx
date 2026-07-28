import { Braces, Hash, Search, Trash2, X } from 'lucide-react'

import { PAGE_SIZES } from './table-model'

/**
 * Search + result count + row-size control, with the bulk-action strip
 * taking over the row whenever a selection exists.
 */
export function TableToolbar({
  filtered,
  onClearSelection,
  onCopyIds,
  onCopyJson,
  onQueryChange,
  onRevokeSelected,
  onSizeChange,
  query,
  selectedCount,
  size,
  total,
}: {
  filtered: number
  onClearSelection: () => void
  onCopyIds: () => void
  onCopyJson: () => void
  onQueryChange: (value: string) => void
  onRevokeSelected: () => void
  onSizeChange: (value: number) => void
  query: string
  selectedCount: number
  size: number
  total: number
}) {
  if (selectedCount > 0) {
    return (
      <div className="border-accent/40 bg-accent/5 flex flex-wrap items-center gap-2 rounded-lg border px-3 py-2">
        <span className="text-sm font-medium tabular-nums">{selectedCount} selected</span>
        <span className="bg-border h-4 w-px" />
        <BulkButton icon={<Hash aria-hidden className="h-3.5 w-3.5" />} onClick={onCopyIds}>
          Copy IDs
        </BulkButton>
        <BulkButton icon={<Braces aria-hidden className="h-3.5 w-3.5" />} onClick={onCopyJson}>
          Copy JSON
        </BulkButton>
        <BulkButton
          danger
          icon={<Trash2 aria-hidden className="h-3.5 w-3.5" />}
          onClick={onRevokeSelected}
        >
          Revoke
        </BulkButton>
        <button
          className="text-fg-muted hover:text-fg ml-auto inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors"
          onClick={onClearSelection}
          type="button"
        >
          <X aria-hidden className="h-3.5 w-3.5" />
          Clear
        </button>
      </div>
    )
  }

  return (
    <div className="flex flex-wrap items-center gap-3">
      <div className="relative w-full min-w-0 sm:max-w-sm">
        <Search
          aria-hidden
          className="text-fg-muted pointer-events-none absolute top-1/2 left-2.5 h-4 w-4 -translate-y-1/2"
        />
        <input
          aria-label="Search API keys"
          className="border-border bg-bg-secondary focus:border-accent focus:ring-accent/30 w-full rounded-md border py-1.5 pr-8 pl-8 text-sm focus:ring-1 focus:outline-none"
          onChange={(e) => onQueryChange(e.target.value)}
          placeholder="Search name, prefix, scope, id"
          type="search"
          value={query}
        />
        {query.length > 0 && (
          <button
            aria-label="Clear search"
            className="text-fg-muted hover:text-fg absolute top-1/2 right-2 -translate-y-1/2"
            onClick={() => onQueryChange('')}
            type="button"
          >
            <X aria-hidden className="h-3.5 w-3.5" />
          </button>
        )}
      </div>

      <span className="text-fg-muted ml-auto text-xs tabular-nums" role="status">
        {countLabel(filtered, total)}
      </span>

      <label className="text-fg-muted flex items-center gap-1.5 text-xs">
        Rows
        <select
          aria-label="Rows per page"
          className="border-border bg-bg-secondary text-fg focus:border-accent rounded-md border px-1.5 py-1 text-xs focus:outline-none"
          onChange={(e) => onSizeChange(Number(e.target.value))}
          value={size}
        >
          {PAGE_SIZES.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      </label>
    </div>
  )
}

function BulkButton({
  children,
  danger = false,
  icon,
  onClick,
}: {
  children: React.ReactNode
  danger?: boolean
  icon: React.ReactNode
  onClick: () => void
}) {
  return (
    <button className={bulkClass(danger)} onClick={onClick} type="button">
      {icon}
      {children}
    </button>
  )
}

function bulkClass(danger: boolean): string {
  const base =
    'inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs transition-colors'
  if (danger) return `${base} border-danger/40 text-danger hover:bg-danger/10`
  return `${base} border-border text-fg-secondary hover:bg-bg-secondary hover:text-fg`
}

function countLabel(filtered: number, total: number): string {
  if (filtered === total) return `${total} keys`
  return `${filtered} of ${total} keys`
}
