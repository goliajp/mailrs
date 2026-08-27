import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { SlidersHorizontal } from 'lucide-react'
import { memo, useEffect, useRef, useState } from 'react'

import { useCategoriesQuery } from '@/hooks/use-mail-queries'
import { MAIL_LIST_TABS, MAIL_LISTS, type MailListId } from '@/lib/mail-lists'
import {
  activeListAtom,
  categoryFilterAtom,
  type ImportanceSection,
  importanceSectionAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectMailListAtom,
  type SortOrder,
  sortOrderAtom,
} from '@/store/ui'

// The tabs are `lib/mail-lists.ts`, in the order it lists them. They
// were a literal here with a `resolveActiveTab()` beside it that
// reconstructed which one was current out of five other atoms; the tab
// is now the state and those atoms are read off it.
//
// 2026-07-16 — two fixed rows (user layout); selection is shown by a
// deeper/solid background (no per-tab colors, no ring) — a uniform
// segmented-control look.
function panelChipClass(isActive: boolean, extra: string): string {
  const base = `rounded-md px-2 py-0.5 transition-colors ${extra}`
  if (isActive) return `${base} bg-fg text-bg`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}

function sectionLabel(s: ImportanceSection): string {
  if (s === null) return 'All'
  if (s === 'important') return 'Important'
  return 'Other'
}

function sortLabel(s: SortOrder): string {
  if (s === 'unread') return 'Unread first'
  if (s === 'relevance') return 'Best match'
  return s
}

// `relevance` is only offered while a search is running — for a plain
// list it means the same thing as `newest`, and a chip that changes
// nothing is worse than one that is absent.
function sortOptions(query: string): SortOrder[] {
  if (query.trim().length > 0) return ['relevance', 'newest', 'oldest', 'unread']
  return ['newest', 'oldest', 'unread']
}

function tabButtonClass(isActive: boolean): string {
  // `w-full` and `truncate`: the cell decides the width now, and a
  // label longer than its column has to cut rather than push the grid
  // out of line. `px-2`, down from `px-3`, so the longest label —
  // `Archived` — still fits its column in a narrow list pane.
  const base = 'w-full cursor-pointer truncate rounded-md px-2 py-1 text-xs transition-colors'
  if (isActive) return `${base} bg-border-strong text-fg font-semibold`
  return `${base} bg-bg-secondary text-fg-muted hover:bg-bg-tertiary hover:text-fg-secondary font-medium`
}

// memo'd because FilterBar takes no props — every parent re-render would
// otherwise re-create the tabs + filter-panel JSX even though the
// atom-backed state is identical.
export const FilterBar = memo(function FilterBar() {
  const activeList = useAtomValue(activeListAtom)
  const selectList = useSetAtom(selectMailListAtom)
  const [section, setSection] = useAtom(importanceSectionAtom)
  const [sortOrder, setSortOrder] = useAtom(sortOrderAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const [activeCategory, setActiveCategory] = useAtom(categoryFilterAtom)
  const [selectedDomains, setSelectedDomains] = useAtom(selectedDomainsAtom)
  const selectedDomainsVal = useAtomValue(selectedDomainsAtom)
  const [filtersOpen, setFiltersOpen] = useState(false)
  const panelRef = useRef<HTMLDivElement>(null)

  const { data: categories = [] } = useCategoriesQuery(selectedDomainsVal)

  useEffect(() => {
    if (!filtersOpen) return
    const handler = (e: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
        setFiltersOpen(false)
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [filtersOpen])

  // Switching lists resets the refinements stacked on the old one: a
  // category chosen inside Inbox is not a question about Junk, and
  // leaving it applied is how a tab could open empty for no visible
  // reason. `selectMailListAtom` owns the list and the pick; these two
  // are the filter bar's own state and it clears them here.
  const handleTab = (id: MailListId) => {
    if (id === activeList) return
    setActiveCategory(null)
    setSection(null)
    selectList(id)
  }

  const hasAdvancedFilters =
    sortOrder !== 'newest' ||
    (activeCategory !== null && activeCategory !== 'spam' && activeCategory !== 'scam') ||
    selectedDomains.length > 0 ||
    section === 'important' ||
    section === 'other'

  let filterBtnClass = 'text-fg-muted hover:bg-bg-secondary'
  if (filtersOpen || hasAdvancedFilters) filterBtnClass = 'text-accent'

  return (
    <div className="border-border flex items-start gap-1 border-b px-3 py-1.5">
      {/* One grid over both rows, not a flex row each. Wrapped flex
          sizes every tab to its own label, so `Inbox` was narrow,
          `Archived` was wide, and the second row started under nothing
          in particular. Five columns give every tab the same width and
          put the second row's three under the first row's three. */}
      <div className="grid flex-1 grid-cols-5 gap-1">
        {MAIL_LIST_TABS.map((id) => (
          <button
            className={tabButtonClass(activeList === id)}
            key={id}
            onClick={() => handleTab(id)}
          >
            {MAIL_LISTS[id].label}
          </button>
        ))}
      </div>

      {/* Beside the filters rather than inside them: which mailboxes
          you are reading is a standing choice, not one of the
          refinements you open a panel to change. It renders nothing
          until there is a second account to leave out. */}

      <div className="relative" ref={panelRef}>
        <button
          aria-label="Toggle filters"
          className={`relative flex h-7 w-7 items-center justify-center rounded-md transition-all duration-150 ${filterBtnClass}`}
          onClick={() => setFiltersOpen((prev) => !prev)}
          title="Filters"
        >
          <SlidersHorizontal className="h-3.5 w-3.5" />
          {hasAdvancedFilters && (
            <span className="bg-accent absolute -top-0.5 -right-0.5 h-2 w-2 rounded-full" />
          )}
        </button>

        {filtersOpen && (
          <div className="border-border bg-surface absolute top-full right-0 z-50 mt-1 w-56 rounded-lg border p-3 text-xs shadow-lg">
            <div className="mb-3">
              <label className="text-fg-muted mb-1 block font-medium">Sort</label>
              <div className="flex gap-1">
                {sortOptions(searchQuery).map((s) => (
                  <button
                    className={panelChipClass(sortOrder === s, 'capitalize')}
                    key={s}
                    onClick={() => setSortOrder(s)}
                  >
                    {sortLabel(s)}
                  </button>
                ))}
              </div>
            </div>

            <div className="mb-3">
              <label className="text-fg-muted mb-1 block font-medium">Priority</label>
              <div className="flex flex-wrap gap-1">
                {([null, 'important', 'other'] as ImportanceSection[]).map((s) => (
                  <button
                    className={panelChipClass(section === s, '')}
                    key={s ?? 'all'}
                    onClick={() => {
                      if (section === s) setSection(null)
                      else setSection(s)
                    }}
                  >
                    {sectionLabel(s)}
                  </button>
                ))}
              </div>
            </div>

            {categories.length > 0 && (
              <div className="mb-3">
                <label className="text-fg-muted mb-1 block font-medium">Category</label>
                <div className="flex flex-wrap gap-1">
                  <button
                    className={panelChipClass(activeCategory === null, '')}
                    onClick={() => setActiveCategory(null)}
                  >
                    All
                  </button>
                  {categories.map((cat) => (
                    <button
                      className={panelChipClass(activeCategory === cat.category, 'capitalize')}
                      key={cat.category}
                      onClick={() => {
                        if (activeCategory === cat.category) setActiveCategory(null)
                        else setActiveCategory(cat.category)
                      }}
                    >
                      {cat.category}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {hasAdvancedFilters && (
              <button
                className="border-border text-fg-muted hover:bg-bg-secondary mt-3 w-full rounded-md border py-1 text-center transition-colors"
                onClick={() => {
                  setSortOrder('newest')
                  setActiveCategory(null)
                  setSelectedDomains([])
                  setSection(null)
                  setFiltersOpen(false)
                }}
              >
                Reset filters
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  )
})
