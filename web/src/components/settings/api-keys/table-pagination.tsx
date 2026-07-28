import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from 'lucide-react'

/** Footer pager. Hidden entirely when everything fits on one page. */
export function TablePagination({
  onPageChange,
  page,
  pages,
  rangeEnd,
  rangeStart,
  total,
}: {
  onPageChange: (page: number) => void
  page: number
  pages: number
  rangeEnd: number
  rangeStart: number
  total: number
}) {
  if (pages <= 1) {
    return (
      <p className="text-fg-muted text-xs tabular-nums">
        Showing {total} of {total}
      </p>
    )
  }

  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <p className="text-fg-muted text-xs tabular-nums">
        Showing {rangeStart}–{rangeEnd} of {total}
      </p>
      <nav aria-label="Pagination" className="flex items-center gap-1">
        <PageButton disabled={page === 1} label="First page" onClick={() => onPageChange(1)}>
          <ChevronsLeft aria-hidden className="h-4 w-4" />
        </PageButton>
        <PageButton
          disabled={page === 1}
          label="Previous page"
          onClick={() => onPageChange(page - 1)}
        >
          <ChevronLeft aria-hidden className="h-4 w-4" />
        </PageButton>
        <span className="text-fg-secondary px-2 text-xs tabular-nums">
          Page {page} / {pages}
        </span>
        <PageButton
          disabled={page === pages}
          label="Next page"
          onClick={() => onPageChange(page + 1)}
        >
          <ChevronRight aria-hidden className="h-4 w-4" />
        </PageButton>
        <PageButton disabled={page === pages} label="Last page" onClick={() => onPageChange(pages)}>
          <ChevronsRight aria-hidden className="h-4 w-4" />
        </PageButton>
      </nav>
    </div>
  )
}

function PageButton({
  children,
  disabled,
  label,
  onClick,
}: {
  children: React.ReactNode
  disabled: boolean
  label: string
  onClick: () => void
}) {
  return (
    <button
      aria-label={label}
      className="border-border text-fg-secondary hover:bg-bg-secondary hover:text-fg rounded-md border p-1 transition-colors disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent"
      disabled={disabled}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  )
}
