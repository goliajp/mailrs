import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { Check, CheckCircle, Mail, Pin, Search, SlidersHorizontal, SquarePen, Star } from 'lucide-react'
import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { toast } from 'sonner'

import { CategoryBadge, ImportanceBadge } from '@/components/category-badge'
import { ContextMenu, useContextMenu } from '@/components/context-menu'
import type { ContextMenuItem } from '@/components/context-menu'
import { fetchJson, postJson, snoozeConversation } from '@/lib/api'
import { avatarColor, avatarInitial, extractName } from '@/lib/avatar'
import { formatDate, formatFullDate } from '@/lib/format'
import type { CategoryCount, ConversationSummary } from '@/lib/types'
import { authAtom } from '@/store/auth'
import {
  batchModeAtom,
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
  crossAccountReadAtom,
  hasMoreAtom,
  importanceSectionAtom,
  initialLoadingAtom,
  loadingMoreAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  selectedThreadIdsAtom,
  folderAtom,
  showArchivedAtom,
  sortOrderAtom,
  visibleConversationIdsAtom,
  quickFilterAtom,
  type ImportanceSection,
  type SortOrder,
} from '@/store/chat'

type BatchAction = 'read' | 'unread' | 'delete' | 'star' | 'unstar' | 'archive' | 'unarchive'
type SingleAction = BatchAction | 'pin' | 'unpin' | 'snooze'

interface BatchResult {
  success: boolean
  processed: number
  failed: number
  message?: string
}

interface ApiResult {
  success: boolean
  message?: string
}

function PreviewCard({ convo, style }: { convo: ConversationSummary; style: React.CSSProperties }) {
  return (
    <div
      style={{ ...style, boxShadow: 'var(--shadow-lg)' }}
      className="pointer-events-none fixed z-50 w-72 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-3"
    >
      <p className="text-xs font-semibold text-[var(--color-text-primary)]">{convo.subject || '(no subject)'}</p>
      <p className="mt-1 text-[11px] text-[var(--color-text-tertiary)]">
        {convo.participants.slice(0, 3).map(p => extractName(p)).join(', ')}
        {convo.participants.length > 3 && ` +${convo.participants.length - 3}`}
      </p>
      {convo.snippet && (
        <p className="mt-1.5 line-clamp-4 text-xs leading-relaxed text-[var(--color-text-secondary)]">{convo.snippet}</p>
      )}
      <div className="mt-2 flex items-center gap-2 text-[11px] text-[var(--color-text-tertiary)]">
        <span>{formatDate(convo.last_date)}</span>
        {convo.unread_count > 0 && <span className="font-medium text-[var(--color-brand-primary)]">{convo.unread_count} unread</span>}
        {convo.message_count > 1 && <span>{convo.message_count} messages</span>}
      </div>
    </div>
  )
}

const ConversationItem = memo(function ConversationItem({
  convo,
  selected,
  batchMode,
  checked,
  onSelect,
  onToggleCheck,
  onContextAction,
}: {
  convo: ConversationSummary
  selected: boolean
  batchMode: boolean
  checked: boolean
  onSelect: (threadId: string) => void
  onToggleCheck: (threadId: string) => void
  onContextAction: (threadId: string, action: SingleAction) => void
}) {
  const firstParticipant = convo.participants[0] ?? ''
  const name = extractName(firstParticipant)
  const initial = avatarInitial(firstParticipant)
  const color = avatarColor(firstParticipant)
  const hasUnread = convo.unread_count > 0
  const isFlagged = convo.flagged
  const isPinned = convo.pinned
  const isArchived = convo.archived

  const ctx = useContextMenu()

  const [showPreview, setShowPreview] = useState(false)
  const [previewPos, setPreviewPos] = useState<{ top: number; left: number }>({ top: 0, left: 0 })
  const hoverTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const handleMouseEnter = useCallback((e: React.MouseEvent) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const top = rect.top + 200 > window.innerHeight ? rect.bottom - 200 : rect.top
    setPreviewPos({ top, left: rect.right + 8 })
    hoverTimer.current = setTimeout(() => setShowPreview(true), 300)
  }, [])

  const handleMouseLeave = useCallback(() => {
    if (hoverTimer.current) clearTimeout(hoverTimer.current)
    hoverTimer.current = null
    setShowPreview(false)
  }, [])

  const contextItems: ContextMenuItem[] = [
    {
      label: hasUnread ? 'Mark as read' : 'Mark as unread',
      onClick: () => onContextAction(convo.thread_id, hasUnread ? 'read' : 'unread'),
    },
    {
      label: isFlagged ? 'Unstar' : 'Star',
      onClick: () => onContextAction(convo.thread_id, isFlagged ? 'unstar' : 'star'),
    },
    {
      label: isPinned ? 'Unpin' : 'Pin',
      onClick: () => onContextAction(convo.thread_id, isPinned ? 'unpin' : 'pin'),
    },
    {
      label: isArchived ? 'Unarchive' : 'Archive',
      onClick: () => onContextAction(convo.thread_id, isArchived ? 'unarchive' : 'archive'),
    },
    {
      label: 'Snooze until tomorrow',
      onClick: () => onContextAction(convo.thread_id, 'snooze'),
    },
    {
      label: 'Delete',
      danger: true,
      onClick: () => onContextAction(convo.thread_id, 'delete'),
    },
  ]

  const handleClick = () => {
    if (batchMode) {
      onToggleCheck(convo.thread_id)
    } else {
      onSelect(convo.thread_id)
    }
  }

  return (
    <div role="listitem" onMouseEnter={handleMouseEnter} onMouseLeave={handleMouseLeave}>
    <button
      onClick={handleClick}
      onContextMenu={ctx.open}
      aria-selected={selected && !batchMode}
      aria-label={`${name}: ${convo.subject || '(no subject)'}${hasUnread ? `, ${convo.unread_count} unread` : ''}${isPinned ? ', pinned' : ''}`}
      className={`relative flex w-full items-start gap-3 px-4 py-3 text-left transition-all duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)] ${
        selected && !batchMode
          ? 'border-l-[4px] border-l-[var(--color-brand-primary)]'
          : hasUnread
            ? 'border-l-[3px] border-l-[var(--color-brand-primary)] opacity-70'
            : 'border-l-[3px] border-l-transparent'
      } ${
        !hasUnread && !selected && !checked ? 'opacity-70 hover:opacity-100' : ''
      } ${
        selected && !batchMode
          ? 'bg-[var(--color-brand-subtle)]'
          : checked
            ? 'bg-[var(--color-brand-subtle)]'
            : 'hover:bg-[var(--color-hover)]'
      }`}
    >
      {batchMode && (
        <div className="mt-0.5 flex shrink-0 items-center">
          <div
            className={`flex h-5 w-5 items-center justify-center rounded border-2 transition-colors ${
              checked
                ? 'border-[var(--color-brand-primary)] bg-[var(--color-brand-primary)]'
                : 'border-[var(--color-border-default)] bg-[var(--color-bg-base)]'
            }`}
          >
            {checked && (
              <Check className="h-3 w-3 text-white" />
            )}
          </div>
        </div>
      )}
      <div
        className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-medium text-white ${color}`}
      >
        {initial}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <span
            className={`truncate text-sm ${hasUnread ? 'font-semibold text-[var(--color-text-primary)]' : 'text-[var(--color-text-secondary)]'}`}
          >
            {name}
            {convo.participants.length > 1 && (
              <span className="text-[var(--color-text-tertiary)]">
                {' '}
                +{convo.participants.length - 1}
              </span>
            )}
          </span>
          <div className="flex shrink-0 items-center gap-1">
            {isPinned && (
              <Pin className="h-3 w-3 text-[var(--color-brand-primary)]" />
            )}
            <span className="text-xs text-[var(--color-text-tertiary)]" title={formatFullDate(convo.last_date)}>
              {formatDate(convo.last_date)}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <p
            className={`min-w-0 flex-1 truncate text-sm ${hasUnread ? 'font-medium text-[var(--color-text-primary)]' : 'text-[var(--color-text-tertiary)]'}`}
          >
            {convo.subject || '(no subject)'}
          </p>
          {isFlagged && (
            <Star className="h-3.5 w-3.5 shrink-0 text-[var(--color-status-warning)]" fill="currentColor" />
          )}
          <ImportanceBadge level={convo.importance_level} />
          {convo.category && convo.category !== 'general' && (
            <CategoryBadge category={convo.category} />
          )}
          {hasUnread && (
            <span className="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-[var(--color-brand-primary)] px-1.5 text-xs font-medium text-white">
              {convo.unread_count}
            </span>
          )}
        </div>
        {convo.snippet && (
          <p className="truncate text-xs text-[var(--color-text-tertiary)]">
            {convo.snippet}
          </p>
        )}
      </div>
    </button>
    <ContextMenu position={ctx.position} items={contextItems} onClose={ctx.close} />
    {showPreview && !batchMode && createPortal(
      <PreviewCard convo={convo} style={{ top: previewPos.top, left: previewPos.left }} />,
      document.body
    )}
    </div>
  )
})

// unified tab bar
const VIEW_TABS: { value: string; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'unread', label: 'Unread' },
  { value: 'starred', label: 'Starred' },
  { value: 'sent', label: 'Sent' },
  { value: 'action', label: 'Action' },
]

function FilterBar() {
  const [quickFilter, setQuickFilter] = useAtom(quickFilterAtom)
  const [folder, setFolder] = useAtom(folderAtom)
  const [section, setSection] = useAtom(importanceSectionAtom)
  const [sortOrder, setSortOrder] = useAtom(sortOrderAtom)
  const [showArchived, setShowArchived] = useAtom(showArchivedAtom)
  const [activeCategory, setActiveCategory] = useAtom(categoryFilterAtom)
  const [selectedDomains, setSelectedDomains] = useAtom(selectedDomainsAtom)
  const [crossAccountRead, setCrossAccountRead] = useAtom(crossAccountReadAtom)
  const auth = useAtomValue(authAtom)
  const selectedDomainsVal = useAtomValue(selectedDomainsAtom)
  const conversations = useAtomValue(conversationsAtom)
  const [categories, setCategories] = useState<CategoryCount[]>([])
  const [filtersOpen, setFiltersOpen] = useState(false)
  const panelRef = useRef<HTMLDivElement>(null)

  // fetch categories
  useEffect(() => {
    const domainsParam = selectedDomainsVal.length > 0
      ? `?domains=${encodeURIComponent(selectedDomainsVal.join(','))}`
      : ''
    fetchJson<CategoryCount[]>(`/conversations/categories${domainsParam}`).then(
      (data) => setCategories(data),
      () => {}
    )
  }, [selectedDomainsVal])

  // close dropdown on outside click
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

  // compute active tab from folder + quickFilter + importanceSection
  const activeTab = folder === 'Sent' ? 'sent' : section === 'action' ? 'action' : quickFilter !== 'all' ? quickFilter : 'all'

  const handleTab = (tab: string) => {
    if (tab === 'sent') {
      setQuickFilter('all')
      setSection(null)
      setFolder(folder === 'Sent' ? null : 'Sent')
    } else if (tab === 'action') {
      setFolder(null)
      setQuickFilter('all')
      setSection(section === 'action' ? null : 'action')
    } else if (tab === 'unread') {
      setFolder(null)
      setSection(null)
      setQuickFilter(quickFilter === 'unread' ? 'all' : 'unread')
    } else if (tab === 'starred') {
      setFolder(null)
      setSection(null)
      setQuickFilter(quickFilter === 'starred' ? 'all' : 'starred')
    } else {
      setFolder(null)
      setQuickFilter('all')
      setSection(null)
    }
  }

  // action count for badge
  const actionCount = useMemo(() =>
    conversations.filter(
      (c) => c.importance_level === 'critical' || (c.importance_level === 'important' && c.unread_count > 0)
    ).length,
    [conversations]
  )

  // whether any advanced filters are active
  const hasAdvancedFilters = sortOrder !== 'newest' || showArchived || activeCategory !== null || selectedDomains.length > 0 || section === 'important' || section === 'other'

  const superDomains = auth?.accessible_domains ?? []

  return (
    <div className="flex items-center gap-1 border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-3 py-1.5">
      {/* main tabs */}
      {VIEW_TABS.map((t) => (
        <button
          key={t.value}
          onClick={() => handleTab(t.value)}
          className={`shrink-0 rounded-full px-2.5 py-1 text-xs font-medium transition-colors ${
            activeTab === t.value
              ? t.value === 'action'
                ? 'bg-[var(--color-status-danger)] text-white'
                : 'bg-[var(--color-bg-inverted)] text-[var(--color-text-on-inverted)]'
              : 'bg-[var(--color-hover)] text-[var(--color-text-tertiary)] hover:bg-[var(--color-active)] hover:text-[var(--color-text-secondary)]'
          }`}
        >
          {t.label}
          {t.value === 'action' && actionCount > 0 && (
            <span className="ml-1 opacity-70">{actionCount}</span>
          )}
        </button>
      ))}

      {/* filter dropdown toggle */}
      <div className="relative ml-auto" ref={panelRef}>
        <button
          onClick={() => setFiltersOpen((prev) => !prev)}
          className={`relative flex h-7 w-7 items-center justify-center rounded-md transition-all duration-150 ${
            filtersOpen || hasAdvancedFilters
              ? 'text-[var(--color-brand-primary)]'
              : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)]'
          }`}
          title="Filters"
          aria-label="Toggle filters"
        >
          <SlidersHorizontal className="h-3.5 w-3.5" />
          {hasAdvancedFilters && (
            <span className="absolute -right-0.5 -top-0.5 h-2 w-2 rounded-full bg-[var(--color-brand-primary)]" />
          )}
        </button>

        {/* filter dropdown panel */}
        {filtersOpen && (
          <div
            style={{ boxShadow: 'var(--shadow-lg)' }}
            className="absolute right-0 top-full z-50 mt-1 w-56 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-3 text-xs"
          >
            {/* sort */}
            <div className="mb-3">
              <label className="mb-1 block font-medium text-[var(--color-text-tertiary)]">Sort</label>
              <div className="flex gap-1">
                {(['newest', 'oldest', 'unread'] as SortOrder[]).map((s) => (
                  <button
                    key={s}
                    onClick={() => setSortOrder(s)}
                    className={`rounded-md px-2 py-0.5 capitalize transition-colors ${
                      sortOrder === s
                        ? 'bg-[var(--color-bg-inverted)] text-[var(--color-text-on-inverted)]'
                        : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                    }`}
                  >
                    {s === 'unread' ? 'Unread first' : s}
                  </button>
                ))}
              </div>
            </div>

            {/* view: active / archived */}
            <div className="mb-3">
              <label className="mb-1 block font-medium text-[var(--color-text-tertiary)]">View</label>
              <div className="flex gap-1">
                <button
                  onClick={() => setShowArchived(false)}
                  className={`rounded-md px-2 py-0.5 transition-colors ${
                    !showArchived
                      ? 'bg-[var(--color-bg-inverted)] text-[var(--color-text-on-inverted)]'
                      : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                  }`}
                >
                  Active
                </button>
                <button
                  onClick={() => setShowArchived(true)}
                  className={`rounded-md px-2 py-0.5 transition-colors ${
                    showArchived
                      ? 'bg-[var(--color-bg-inverted)] text-[var(--color-text-on-inverted)]'
                      : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                  }`}
                >
                  Archived
                </button>
              </div>
            </div>

            {/* priority */}
            <div className="mb-3">
              <label className="mb-1 block font-medium text-[var(--color-text-tertiary)]">Priority</label>
              <div className="flex flex-wrap gap-1">
                {([null, 'important', 'other'] as ImportanceSection[]).map((s) => (
                  <button
                    key={s ?? 'all'}
                    onClick={() => setSection(section === s ? null : s)}
                    className={`rounded-md px-2 py-0.5 transition-colors ${
                      section === s
                        ? 'bg-[var(--color-bg-inverted)] text-[var(--color-text-on-inverted)]'
                        : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                    }`}
                  >
                    {s === null ? 'All' : s === 'important' ? 'Important' : 'Other'}
                  </button>
                ))}
              </div>
            </div>

            {/* categories */}
            {categories.length > 0 && (
              <div className="mb-3">
                <label className="mb-1 block font-medium text-[var(--color-text-tertiary)]">Category</label>
                <div className="flex flex-wrap gap-1">
                  <button
                    onClick={() => setActiveCategory(null)}
                    className={`rounded-md px-2 py-0.5 transition-colors ${
                      activeCategory === null
                        ? 'bg-[var(--color-bg-inverted)] text-[var(--color-text-on-inverted)]'
                        : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                    }`}
                  >
                    All
                  </button>
                  {categories.map((cat) => (
                    <button
                      key={cat.category}
                      onClick={() => setActiveCategory(activeCategory === cat.category ? null : cat.category)}
                      className={`rounded-md px-2 py-0.5 capitalize transition-colors ${
                        activeCategory === cat.category
                          ? 'bg-[var(--color-bg-inverted)] text-[var(--color-text-on-inverted)]'
                          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                      }`}
                    >
                      {cat.category}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {/* domains (super-admin only) */}
            {superDomains.length > 0 && (
              <div>
                <label className="mb-1 block font-medium text-[var(--color-text-tertiary)]">Domains</label>
                <div className="flex flex-wrap gap-1">
                  <button
                    onClick={() => setSelectedDomains([])}
                    className={`rounded-md px-2 py-0.5 transition-colors ${
                      selectedDomains.length === 0
                        ? 'bg-[var(--color-brand-primary)] text-white'
                        : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                    }`}
                  >
                    Mine
                  </button>
                  {superDomains.map((d) => (
                    <button
                      key={d}
                      onClick={() => setSelectedDomains((prev) =>
                        prev.includes(d) ? prev.filter((x) => x !== d) : [...prev, d]
                      )}
                      className={`rounded-md px-2 py-0.5 transition-colors ${
                        selectedDomains.includes(d)
                          ? 'bg-[var(--color-brand-primary)] text-white'
                          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                      }`}
                    >
                      {d}
                    </button>
                  ))}
                </div>
                {selectedDomains.length > 0 && (
                  <label className="mt-1.5 flex cursor-pointer items-center gap-1 text-[var(--color-text-tertiary)]">
                    <input
                      type="checkbox"
                      checked={crossAccountRead}
                      onChange={(e) => setCrossAccountRead(e.target.checked)}
                      className="h-3 w-3 rounded border-[var(--color-border-default)] accent-[var(--color-brand-primary)]"
                    />
                    Cross-read
                  </label>
                )}
              </div>
            )}

            {/* reset all filters */}
            {hasAdvancedFilters && (
              <button
                onClick={() => {
                  setSortOrder('newest')
                  setShowArchived(false)
                  setActiveCategory(null)
                  setSelectedDomains([])
                  setSection(null)
                  setFiltersOpen(false)
                }}
                className="mt-3 w-full rounded-md border border-[var(--color-border-default)] py-1 text-center text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
              >
                Reset filters
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

function dateLabel(epoch: number): string {
  const d = new Date(epoch * 1000)
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const msgDate = new Date(d.getFullYear(), d.getMonth(), d.getDate())
  const diffDays = Math.floor((today.getTime() - msgDate.getTime()) / 86400000)

  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays < 7) return d.toLocaleDateString(undefined, { weekday: 'long' })
  return d.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    year: now.getFullYear() !== d.getFullYear() ? 'numeric' : undefined,
  })
}

function DateDivider({ label }: { label: string }) {
  return (
    <div className="sticky top-0 z-10 flex select-none justify-center py-1.5">
      <span className="rounded-full bg-[var(--color-bg-sunken)] px-2.5 py-0.5 text-[10px] font-medium text-[var(--color-text-tertiary)]">{label}</span>
    </div>
  )
}

function ConversationSkeleton() {
  return (
    <div className="animate-pulse">
      {Array.from({ length: 8 }).map((_, i) => (
        <div key={i} className="flex items-start gap-3 px-4 py-3">
          <div className="h-9 w-9 shrink-0 rounded-full bg-[var(--color-border-default)]" />
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex items-center justify-between">
              <div className="h-3.5 w-24 rounded bg-[var(--color-border-default)]" />
              <div className="h-3 w-10 rounded bg-[var(--color-border-default)]" />
            </div>
            <div className="h-3 w-40 rounded bg-[var(--color-border-default)]" />
          </div>
        </div>
      ))}
    </div>
  )
}


// floating action bar at bottom of list during batch mode
function BatchActionBar({
  selectedCount,
  onAction,
  onCancel,
  loading,
}: {
  selectedCount: number
  onAction: (action: BatchAction) => void
  onCancel: () => void
  loading: boolean
}) {
  return (
    <div className="absolute bottom-0 left-0 right-0 z-40 border-t border-[var(--color-border-default)] bg-[var(--color-bg-overlay)] px-3 py-2 backdrop-blur">
      <div className="flex items-center gap-2">
        <span className="shrink-0 text-xs font-medium text-[var(--color-text-secondary)]">
          {selectedCount} selected
        </span>
        <div className="flex flex-1 items-center gap-1.5 overflow-x-auto">
          <button
            onClick={() => onAction('read')}
            disabled={loading}
            className="shrink-0 rounded-md px-2.5 py-1 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)]"
          >
            Mark read
          </button>
          <button
            onClick={() => onAction('unread')}
            disabled={loading}
            className="shrink-0 rounded-md px-2.5 py-1 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)]"
          >
            Mark unread
          </button>
          <button
            onClick={() => onAction('star')}
            disabled={loading}
            className="shrink-0 rounded-md px-2.5 py-1 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)]"
          >
            Star
          </button>
          <button
            onClick={() => onAction('archive')}
            disabled={loading}
            className="shrink-0 rounded-md px-2.5 py-1 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)]"
          >
            Archive
          </button>
          <button
            onClick={() => onAction('delete')}
            disabled={loading}
            className="shrink-0 rounded-md px-2.5 py-1 text-xs font-medium text-[var(--color-status-danger)] transition-colors hover:bg-[var(--color-status-danger-subtle)] disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)]"
          >
            Delete
          </button>
        </div>
        <button
          onClick={onCancel}
          disabled={loading}
          className="shrink-0 rounded-md px-2.5 py-1 text-xs font-medium text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
        >
          Cancel
        </button>
        {loading && (
          <div className="h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-[var(--color-border-default)] border-t-[var(--color-text-secondary)]" />
        )}
      </div>
    </div>
  )
}

export function ConversationList({ onLoadMore, onSelectConversation }: { onLoadMore: () => void; onSelectConversation?: () => void }) {
  const conversations = useAtomValue(conversationsAtom)
  const [selectedId, setSelectedId] = useAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const [searchQuery, setSearchQuery] = useAtom(searchQueryAtom)
  const hasMore = useAtomValue(hasMoreAtom)
  const loadingMore = useAtomValue(loadingMoreAtom)
  const initialLoading = useAtomValue(initialLoadingAtom)

  // batch mode state
  const [batchMode, setBatchMode] = useAtom(batchModeAtom)
  const [selectedThreadIds, setSelectedThreadIds] = useAtom(selectedThreadIdsAtom)
  const [batchLoading, setBatchLoading] = useState(false)

  // refs to avoid stale closures in observer callback
  const onLoadMoreRef = useRef(onLoadMore)
  onLoadMoreRef.current = onLoadMore
  const loadingRef = useRef(loadingMore)
  loadingRef.current = loadingMore

  // observer ref to clean up when sentinel unmounts
  const observerRef = useRef<IntersectionObserver | null>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  // callback ref: called when sentinel mounts/unmounts
  const sentinelCallback = useCallback((node: HTMLDivElement | null) => {
    // disconnect old observer
    if (observerRef.current) {
      observerRef.current.disconnect()
      observerRef.current = null
    }

    if (!node) return

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !loadingRef.current) {
          onLoadMoreRef.current()
        }
      },
      {
        root: scrollContainerRef.current,
        rootMargin: '300px',
      }
    )
    observer.observe(node)
    observerRef.current = observer
  }, [])

  // cleanup on unmount
  useEffect(() => {
    return () => {
      observerRef.current?.disconnect()
    }
  }, [])

  // exit batch mode and clear selection
  const exitBatchMode = useCallback(() => {
    setBatchMode(false)
    setSelectedThreadIds(new Set())
  }, [setBatchMode, setSelectedThreadIds])

  // toggle individual thread in selection set
  const toggleThreadCheck = useCallback((threadId: string) => {
    setSelectedThreadIds((prev) => {
      const next = new Set(prev)
      if (next.has(threadId)) {
        next.delete(threadId)
      } else {
        next.add(threadId)
      }
      return next
    })
  }, [setSelectedThreadIds])

  // execute batch action against API then refresh
  const handleBatchAction = useCallback(async (action: BatchAction) => {
    const ids = Array.from(selectedThreadIds)
    if (ids.length === 0) return

    setBatchLoading(true)
    try {
      const result = await postJson<BatchResult>('/conversations/batch', {
        thread_ids: ids,
        action,
      })
      const msg = result.message ?? (result.success ? 'Done' : 'Some operations failed')
      if (result.success) {
        toast.success(msg)
      } else {
        toast.error(msg)
      }
      exitBatchMode()
      // trigger list refresh
      onLoadMoreRef.current()
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Batch operation failed')
    } finally {
      setBatchLoading(false)
    }
  }, [selectedThreadIds, exitBatchMode])

  // single-thread context menu action: pin/unpin/archive/unarchive use dedicated endpoints, others use batch API
  const handleContextAction = useCallback(async (threadId: string, action: SingleAction) => {
    try {
      if (action === 'snooze') {
        const tomorrow = new Date()
        tomorrow.setDate(tomorrow.getDate() + 1)
        tomorrow.setHours(9, 0, 0, 0)
        await snoozeConversation(threadId, tomorrow.toISOString())
        toast.success('Snoozed until tomorrow 9:00')
      } else if (action === 'pin' || action === 'unpin' || action === 'archive' || action === 'unarchive') {
        await postJson<ApiResult>(`/conversations/${encodeURIComponent(threadId)}/${action}`, {})
        const labels: Record<string, string> = { pin: 'Pinned', unpin: 'Unpinned', archive: 'Archived', unarchive: 'Unarchived' }
        toast.success(labels[action] ?? 'Updated')
      } else {
        await postJson<BatchResult>('/conversations/batch', {
          thread_ids: [threadId],
          action,
        })
        toast.success(action === 'delete' ? 'Deleted' : 'Updated')
      }
      onLoadMoreRef.current()
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Action failed')
    }
  }, [])

  const sortOrder = useAtomValue(sortOrderAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const quickFilter = useAtomValue(quickFilterAtom)

  // apply client-side filtering + sort
  const sortedConversations = useMemo(() => {
    let visible = showArchived ? conversations : conversations.filter((c) => !c.archived)

    // quick filter
    if (quickFilter === 'unread') {
      visible = visible.filter((c) => c.unread_count > 0)
    } else if (quickFilter === 'starred') {
      visible = visible.filter((c) => c.flagged)
    }
    // attachment filter skipped: ConversationSummary does not have has_attachments yet

    // importance section filter
    if (importanceSection === 'action') {
      visible = visible.filter(
        (c) => c.importance_level === 'critical' || (c.importance_level === 'important' && c.unread_count > 0)
      )
    } else if (importanceSection === 'important') {
      visible = visible.filter(
        (c) => c.importance_level === 'critical' || c.importance_level === 'important'
      )
    } else if (importanceSection === 'other') {
      visible = visible.filter(
        (c) => c.importance_level === 'low' || c.importance_level === 'noise'
      )
    }

    if (sortOrder === 'newest') return visible
    const pinned = visible.filter((c) => c.pinned)
    const unpinned = visible.filter((c) => !c.pinned)
    if (sortOrder === 'oldest') {
      unpinned.sort((a, b) => a.last_date - b.last_date)
    } else if (sortOrder === 'unread') {
      unpinned.sort((a, b) => b.unread_count - a.unread_count || b.last_date - a.last_date)
    }
    return [...pinned, ...unpinned]
  }, [conversations, sortOrder, showArchived, importanceSection, quickFilter])

  // sync visible conversation ids to store for keyboard nav
  const setVisibleIds = useSetAtom(visibleConversationIdsAtom)
  useEffect(() => {
    setVisibleIds(sortedConversations.map((c) => c.thread_id))
  }, [sortedConversations, setVisibleIds])

  // stable callbacks that accept threadId to avoid inline closures in the map
  const handleSelect = useCallback((threadId: string) => {
    setSelectedId(threadId)
    setComposingNew(false)
    onSelectConversation?.()
  }, [setSelectedId, setComposingNew, onSelectConversation])

  const isSearching = searchQuery.trim().length > 0
  const hasBatchBar = batchMode && selectedThreadIds.size > 0

  return (
    <div className="relative flex h-full select-none flex-col">
      <div className="flex items-center gap-2 border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-3">
        <div role="search" className="relative flex-1">
          <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--color-text-tertiary)]" aria-hidden="true" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            aria-label="Search conversations"
            className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] py-1.5 pl-9 pr-3 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-border-strong)]"
          />
        </div>

        {/* batch select toggle — hidden during search */}
        {!isSearching && (
          <button
            onClick={() => {
              if (batchMode) {
                exitBatchMode()
              } else {
                setBatchMode(true)
              }
            }}
            aria-pressed={batchMode}
            aria-label={batchMode ? 'Exit batch select mode' : 'Enter batch select mode'}
            className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150 ${
              batchMode
                ? 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'
                : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)]'
            }`}
            title="Batch select"
          >
            <CheckCircle className="h-5 w-5" aria-hidden="true" />
          </button>
        )}

        <button
          onClick={() => {
            setComposingNew(true)
            setSelectedId(null)
          }}
          aria-label="New conversation"
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-all duration-150 hover:bg-[var(--color-hover)]"
          title="New conversation"
        >
          <SquarePen className="h-5 w-5" aria-hidden="true" />
        </button>
      </div>

      <FilterBar />

      <div
        ref={scrollContainerRef}
        role="list"
        aria-label="Conversations"
        className={`flex-1 overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
      >
        {initialLoading && conversations.length === 0 ? (
          <ConversationSkeleton />
        ) : conversations.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-8 text-center text-[var(--color-text-tertiary)]">
            <Mail className="mb-3 h-10 w-10 text-[var(--color-text-tertiary)]" strokeWidth={1} aria-hidden="true" />
            <p className="text-sm font-medium">{isSearching ? 'No results found' : 'All caught up!'}</p>
            <p className="mt-1 text-xs">{isSearching ? 'Try a different search term' : 'No conversations to show'}</p>
          </div>
        ) : (
          (() => {
            let prevGroup = ''
            return sortedConversations.map((c) => {
              const group = dateLabel(c.last_date)
              const showDivider = group !== prevGroup
              prevGroup = group
              return (
                <Fragment key={c.thread_id}>
                  {showDivider && <DateDivider label={group} />}
                  <ConversationItem
                    convo={c}
                    selected={selectedId === c.thread_id}
                    batchMode={batchMode}
                    checked={selectedThreadIds.has(c.thread_id)}
                    onSelect={handleSelect}
                    onToggleCheck={toggleThreadCheck}
                    onContextAction={handleContextAction}
                  />
                </Fragment>
              )
            })
          })()
        )}

        {/* infinite scroll sentinel */}
        {hasMore && conversations.length > 0 && (
          <div ref={sentinelCallback} className="flex justify-center py-4">
            {loadingMore && (
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-[var(--color-border-default)] border-t-[var(--color-text-secondary)]" />
            )}
          </div>
        )}

        {!hasMore && conversations.length > 0 && (
          <div className="py-3 text-center text-xs text-[var(--color-text-tertiary)]">
            No more conversations
          </div>
        )}
      </div>

      {/* floating batch action bar */}
      {hasBatchBar && (
        <BatchActionBar
          selectedCount={selectedThreadIds.size}
          onAction={handleBatchAction}
          onCancel={exitBatchMode}
          loading={batchLoading}
        />
      )}
    </div>
  )
}
