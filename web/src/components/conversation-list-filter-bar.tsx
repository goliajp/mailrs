import { useAtom, useAtomValue } from 'jotai'
import { SlidersHorizontal } from 'lucide-react'
import { memo, useEffect, useRef, useState } from 'react'

import { useCategoriesQuery } from '@/hooks/use-mail-queries'
import {
  categoryFilterAtom,
  folderAtom,
  type ImportanceSection,
  importanceSectionAtom,
  type MailFolder,
  quickFilterAtom,
  selectedDomainsAtom,
  showArchivedAtom,
  type SortOrder,
  sortOrderAtom,
} from '@/store/ui'

// v2.4.2 Phase 4.3 — the pre-Phase-2 "Spam" tab is superseded by the
// Junk folder tab. The internal `activeCategory === 'spam'` is still
// honored by the backend for legacy links / stored filters.
// v2.8.2 — "All" becomes "Inbox" (dedicated inbox folder axis: no Junk,
// no sent-only threads). "Archived" is a first-class tab.
// v2.9 triage — "N & P" is the merged Notifications & Promotions view.
// 2026-07-16 — two fixed rows (user layout); selection is shown by a
// deeper/solid background (no per-tab colors, no ring) — a uniform
// segmented-control look.
const TAB_ROWS: { label: string; value: string }[][] = [
  [
    { label: 'Inbox', value: 'inbox' },
    { label: 'N & P', value: 'np' },
    { label: 'Unread', value: 'unread' },
    { label: 'Starred', value: 'starred' },
    { label: 'Junk', value: 'junk' },
  ],
  [
    { label: 'Sent', value: 'sent' },
    { label: 'Draft', value: 'draft' },
    { label: 'Archived', value: 'archived' },
  ],
]

function panelChipClass(isActive: boolean, extra: string): string {
  const base = `rounded-md px-2 py-0.5 transition-colors ${extra}`
  if (isActive) return `${base} bg-fg text-bg`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}

// which tab is highlighted, derived from the several independent atoms
// that together describe the current view. explicit if-returns — no
// nested ternaries.
function resolveActiveTab(state: {
  activeCategory: null | string
  folder: MailFolder
  quickFilter: string
  showArchived: boolean
}): string {
  if (state.activeCategory === 'spam' || state.activeCategory === 'scam') return 'junk'
  if (state.folder === 'Sent') return 'sent'
  if (state.folder === 'Drafts') return 'draft'
  if (state.folder === 'Junk') return 'junk'
  if (state.folder === 'NP') return 'np'
  if (state.showArchived) return 'archived'
  if (state.quickFilter !== 'all') return state.quickFilter
  return 'inbox'
}

function sectionLabel(s: ImportanceSection): string {
  if (s === null) return 'All'
  if (s === 'important') return 'Important'
  return 'Other'
}

function sortLabel(s: SortOrder): string {
  if (s === 'unread') return 'Unread first'
  return s
}

function tabButtonClass(isActive: boolean): string {
  const base = 'shrink-0 cursor-pointer rounded-md px-3 py-1 text-xs transition-colors'
  if (isActive) return `${base} bg-border-strong text-fg font-semibold`
  return `${base} bg-bg-secondary text-fg-muted hover:bg-bg-tertiary hover:text-fg-secondary font-medium`
}

// memo'd because FilterBar takes no props — every parent re-render would
// otherwise re-create the tabs + filter-panel JSX even though the
// atom-backed state is identical.
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

  const activeTab = resolveActiveTab({ activeCategory, folder, quickFilter, showArchived })

  const handleTab = (tab: string) => {
    if (tab === activeTab) return
    // reset to the Inbox base axis, then apply the tab-specific view.
    setActiveCategory(null)
    setFolder('Inbox')
    setQuickFilter('all')
    setSection(null)
    setShowArchived(false)
    switch (tab) {
      case 'archived':
        setShowArchived(true)
        break
      case 'draft':
        setFolder('Drafts')
        break
      case 'junk':
        setFolder('Junk')
        break
      case 'np':
        setFolder('NP')
        break
      case 'sent':
        setFolder('Sent')
        break
      case 'starred':
        setQuickFilter('starred')
        break
      case 'unread':
        setQuickFilter('unread')
        break
      default:
        // 'inbox' — base state already applied above
        break
    }
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
      <div className="flex flex-1 flex-col gap-1">
        {TAB_ROWS.map((row, ri) => (
          // index key OK: static two-row layout, never reordered
          <div className="flex flex-wrap items-center gap-1" key={ri}>
            {row.map((t) => (
              <button
                className={tabButtonClass(activeTab === t.value)}
                key={t.value}
                onClick={() => handleTab(t.value)}
              >
                {t.label}
              </button>
            ))}
          </div>
        ))}
      </div>

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
                {(['newest', 'oldest', 'unread'] as SortOrder[]).map((s) => (
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
