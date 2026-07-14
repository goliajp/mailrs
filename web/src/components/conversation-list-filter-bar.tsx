import { useAtom, useAtomValue } from 'jotai'
import { SlidersHorizontal } from 'lucide-react'
import { memo, useEffect, useRef, useState } from 'react'

import { useCategoriesQuery } from '@/hooks/use-mail-queries'
import {
  categoryFilterAtom,
  folderAtom,
  type ImportanceSection,
  importanceSectionAtom,
  quickFilterAtom,
  selectedDomainsAtom,
  showArchivedAtom,
  type SortOrder,
  sortOrderAtom,
} from '@/store/ui'

// v2.4.2 Phase 4.3 — the pre-Phase-2 "Spam" tab (which set
// `activeCategory='spam'` and filtered on the by_category:spam zset)
// is superseded by the Junk folder tab. Both showed AI-classified
// junk mail; Junk is the top-level folder, Spam was a subfilter on
// Inbox that shared the same threads post-Phase-2 anyway. Keeping
// only Junk removes user confusion about which one to click. The
// internal `activeCategory === 'spam'` is still honored by the
// backend for legacy links / stored filters.
// v2.8.2 — "All" (mixed by_activity axis that leaked Junk + Sent
// threads into the default view) becomes "Inbox" (dedicated inbox
// folder axis: no Junk, no sent-only threads, starred included).
// "Archived" is promoted from the advanced-filter panel toggle to a
// first-class tab; delete inside it is the permanent delete it always
// was on the backend.
const VIEW_TABS: { label: string; value: string }[] = [
  { label: 'Inbox', value: 'inbox' },
  { label: 'Unread', value: 'unread' },
  { label: 'Starred', value: 'starred' },
  { label: 'Archived', value: 'archived' },
  { label: 'Sent', value: 'sent' },
  { label: 'Junk', value: 'junk' },
]

// memo'd because FilterBar takes no props — every parent re-render
// (search box keystroke, selection change, batch-mode toggle) would
// otherwise re-create its 7 tabs + filter-panel JSX even though the
// atom-backed state is identical. With memo, props-equal short-circuit
// makes the function a no-op unless one of its atoms moves; useAtom
// then re-renders only when that specific atom's value changes.
export const FilterBar = memo(function FilterBar() {
  const [quickFilter, setQuickFilter] = useAtom(quickFilterAtom)
  const [folder, setFolder] = useAtom(folderAtom)
  const [section, setSection] = useAtom(importanceSectionAtom)
  const [sortOrder, setSortOrder] = useAtom(sortOrderAtom)
  const [showArchived, setShowArchived] = useAtom(showArchivedAtom)
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

  const activeTab =
    // v2.4.2 Phase 4.3: legacy spam / scam category filters land on
    // the Junk tab so the UI collapses to a single spam-adjacent
    // control.
    activeCategory === 'spam' || activeCategory === 'scam'
      ? 'junk'
      : folder === 'Sent'
        ? 'sent'
        : folder === 'Junk'
          ? 'junk'
          : showArchived
            ? 'archived'
            : quickFilter !== 'all'
              ? quickFilter
              : 'inbox'

  const handleTab = (tab: string) => {
    if (tab === activeTab) return
    setActiveCategory(null)
    // v2.8.2: Inbox is the base axis — Unread / Starred / Archived
    // stack on top of it (backend ZINTERSTOREs the inbox zset with
    // the matching flag index), so none of them leak Junk / Sent.
    setFolder('Inbox')
    setQuickFilter('all')
    setSection(null)
    setShowArchived(false)
    if (tab === 'sent') {
      setFolder('Sent')
    } else if (tab === 'junk') {
      setFolder('Junk')
    } else if (tab === 'archived') {
      setShowArchived(true)
    } else if (tab === 'unread') {
      setQuickFilter('unread')
    } else if (tab === 'starred') {
      setQuickFilter('starred')
    }
  }

  const hasAdvancedFilters =
    sortOrder !== 'newest' ||
    (activeCategory !== null && activeCategory !== 'spam' && activeCategory !== 'scam') ||
    selectedDomains.length > 0 ||
    section === 'important' ||
    section === 'other'

  return (
    <div className="border-border flex items-center gap-1 border-b px-3 py-1.5">
      <div className="scrollbar-hide flex snap-x snap-mandatory items-center gap-1 overflow-x-auto scroll-smooth md:overflow-x-visible">
        {VIEW_TABS.map((t) => {
          const isActive = activeTab === t.value
          const base =
            'snap-start shrink-0 rounded-md px-3 py-1 text-xs font-medium transition-colors cursor-pointer'
          const color =
            t.value === 'junk'
              ? 'bg-danger/10 text-danger'
              : t.value === 'starred'
                ? 'bg-warning/10 text-warning'
                : t.value === 'sent'
                  ? 'bg-success/10 text-success'
                  : t.value === 'unread'
                    ? 'bg-accent/10 text-accent'
                    : 'bg-border text-fg-secondary'
          const ring = isActive ? 'ring-2 ring-offset-1 ring-border ring-offset-bg' : ''
          return (
            <button
              className={`${base} ${color} ${ring}`}
              key={t.value}
              onClick={() => handleTab(t.value)}
            >
              {t.label}
            </button>
          )
        })}
      </div>

      <div className="relative ml-auto" ref={panelRef}>
        <button
          aria-label="Toggle filters"
          className={`relative flex h-7 w-7 items-center justify-center rounded-md transition-all duration-150 ${
            filtersOpen || hasAdvancedFilters
              ? 'text-accent'
              : 'text-fg-muted hover:bg-bg-secondary'
          }`}
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
                {(['newest', 'oldest', 'unread'] as SortOrder[]).map((s) => (
                  <button
                    className={`rounded-md px-2 py-0.5 capitalize transition-colors ${
                      sortOrder === s ? 'bg-fg text-bg' : 'text-fg-secondary hover:bg-bg-secondary'
                    }`}
                    key={s}
                    onClick={() => setSortOrder(s)}
                  >
                    {s === 'unread' ? 'Unread first' : s}
                  </button>
                ))}
              </div>
            </div>

            <div className="mb-3">
              <label className="text-fg-muted mb-1 block font-medium">Priority</label>
              <div className="flex flex-wrap gap-1">
                {([null, 'important', 'other'] as ImportanceSection[]).map((s) => (
                  <button
                    className={`rounded-md px-2 py-0.5 transition-colors ${
                      section === s ? 'bg-fg text-bg' : 'text-fg-secondary hover:bg-bg-secondary'
                    }`}
                    key={s ?? 'all'}
                    onClick={() => setSection(section === s ? null : s)}
                  >
                    {s === null ? 'All' : s === 'important' ? 'Important' : 'Other'}
                  </button>
                ))}
              </div>
            </div>

            {categories.length > 0 && (
              <div className="mb-3">
                <label className="text-fg-muted mb-1 block font-medium">Category</label>
                <div className="flex flex-wrap gap-1">
                  <button
                    className={`rounded-md px-2 py-0.5 transition-colors ${
                      activeCategory === null
                        ? 'bg-fg text-bg'
                        : 'text-fg-secondary hover:bg-bg-secondary'
                    }`}
                    onClick={() => setActiveCategory(null)}
                  >
                    All
                  </button>
                  {categories.map((cat) => (
                    <button
                      className={`rounded-md px-2 py-0.5 capitalize transition-colors ${
                        activeCategory === cat.category
                          ? 'bg-fg text-bg'
                          : 'text-fg-secondary hover:bg-bg-secondary'
                      }`}
                      key={cat.category}
                      onClick={() =>
                        setActiveCategory(activeCategory === cat.category ? null : cat.category)
                      }
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
                  setShowArchived(false)
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
