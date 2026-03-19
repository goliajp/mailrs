import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { Check, CheckCircle, Mail, MailCheck, Pin, Search, SlidersHorizontal, SquarePen, Star, X } from 'lucide-react'
import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { toast } from 'sonner'

import { CategoryBadge, ImportanceBadge } from '@/components/category-badge'
import { ContextMenu, useContextMenu } from '@/components/context-menu'
import type { ContextMenuItem } from '@/components/context-menu'
import { fetchJson, postJson, snoozeConversation } from '@/lib/api'
import { extractEmail, extractName } from '@/lib/avatar'
import { SenderAvatar } from '@/components/sender-avatar'
import { formatDate, formatFullDate } from '@/lib/format'
import type { CategoryCount, ConversationSummary } from '@/lib/types'
import {
  batchModeAtom,
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
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
import { authAtom } from '@/store/auth'

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

const ConversationItem = memo(function ConversationItem({
  convo,
  selected,
  batchMode,
  checked,
  onSelect,
  onToggleCheck,
  onContextAction,
  myEmail,
}: {
  convo: ConversationSummary
  selected: boolean
  batchMode: boolean
  checked: boolean
  onSelect: (threadId: string) => void
  onToggleCheck: (threadId: string) => void
  onContextAction: (threadId: string, action: SingleAction) => void
  myEmail: string
}) {
  const firstParticipant = convo.participants[0] ?? ''
  const firstEmail = extractEmail(firstParticipant)
  const isOwn = firstEmail === myEmail
  const name = isOwn ? 'Me' : extractName(firstParticipant)
  const hasUnread = convo.unread_count > 0
  const isFlagged = convo.flagged
  const isPinned = convo.pinned
  const isArchived = convo.archived

  const ctx = useContextMenu()

  const handleMouseEnter = useCallback(() => {}, [])
  const handleMouseLeave = useCallback(() => {}, [])

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
      className={`relative flex w-full items-start gap-3 px-4 py-2.5 text-left transition-all duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)] ${
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
      <SenderAvatar sender={firstParticipant} size={36} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <span
            className={`truncate text-sm ${isOwn ? 'text-[var(--color-brand-primary)]' : ''} ${hasUnread ? 'font-semibold text-[var(--color-text-primary)]' : isOwn ? '' : 'text-[var(--color-text-secondary)]'}`}
          >
            {name}
            {convo.participants.length > 1 && (
              <span className="text-[var(--color-text-tertiary)]">
                {' '}
                +{convo.participants.length - 1}
              </span>
            )}
          </span>
          <div className="flex shrink-0 items-center gap-1.5">
            {convo.message_count > 1 && (
              <span className="rounded bg-[var(--color-bg-sunken)] px-1 py-px text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
                {convo.message_count}
              </span>
            )}
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
  { value: 'spam', label: 'Spam' },
]

function FilterBar() {
  const [quickFilter, setQuickFilter] = useAtom(quickFilterAtom)
  const [folder, setFolder] = useAtom(folderAtom)
  const [section, setSection] = useAtom(importanceSectionAtom)
  const [sortOrder, setSortOrder] = useAtom(sortOrderAtom)
  const [showArchived, setShowArchived] = useAtom(showArchivedAtom)
  const [activeCategory, setActiveCategory] = useAtom(categoryFilterAtom)
  const [selectedDomains, setSelectedDomains] = useAtom(selectedDomainsAtom)
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

  // compute active tab from folder + quickFilter + importanceSection + category
  const activeTab = activeCategory === 'spam' || activeCategory === 'scam' ? 'spam'
    : folder === 'Sent' ? 'sent'
    : section === 'action' ? 'action'
    : quickFilter !== 'all' ? quickFilter
    : 'all'

  const handleTab = (tab: string) => {
    if (tab === 'spam') {
      setFolder(null)
      setQuickFilter('all')
      setSection(null)
      setActiveCategory(activeCategory === 'spam' ? null : 'spam')
    } else if (tab === 'sent') {
      setActiveCategory(null)
      setQuickFilter('all')
      setSection(null)
      setFolder(folder === 'Sent' ? null : 'Sent')
    } else if (tab === 'action') {
      setActiveCategory(null)
      setFolder(null)
      setQuickFilter('all')
      setSection(section === 'action' ? null : 'action')
    } else if (tab === 'unread') {
      setActiveCategory(null)
      setFolder(null)
      setSection(null)
      setQuickFilter(quickFilter === 'unread' ? 'all' : 'unread')
    } else if (tab === 'starred') {
      setActiveCategory(null)
      setFolder(null)
      setSection(null)
      setQuickFilter(quickFilter === 'starred' ? 'all' : 'starred')
    } else {
      setActiveCategory(null)
      setFolder(null)
      setQuickFilter('all')
      setSection(null)
    }
  }

  // action count for badge
  const [actionCount, setActionCount] = useState(0)
  useEffect(() => {
    const doms = selectedDomainsVal.length > 0 ? `?domains=${encodeURIComponent(selectedDomainsVal.join(','))}` : ''
    fetchJson<{ count: number }>(`/conversations/action-count${doms}`).then(
      (d) => setActionCount(d.count),
      () => {}
    )
  }, [selectedDomainsVal, conversations])

  // whether any advanced filters are active
  const hasAdvancedFilters = sortOrder !== 'newest' || showArchived || (activeCategory !== null && activeCategory !== 'spam' && activeCategory !== 'scam') || selectedDomains.length > 0 || section === 'important' || section === 'other'

  return (
    <div className="flex items-center gap-1 border-b border-[var(--color-border-default)] px-3 py-1.5">
      {/* main tabs */}
      {VIEW_TABS.map((t) => {
        const isActive = activeTab === t.value
        const base = 'shrink-0 rounded-md px-3 py-1 text-xs font-medium transition-colors cursor-pointer'
        const color = t.value === 'spam'
          ? 'bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]'
          : t.value === 'action'
          ? 'bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]'
          : t.value === 'starred'
            ? 'bg-[var(--color-status-warning-subtle)] text-[var(--color-status-warning)]'
            : t.value === 'sent'
              ? 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]'
              : t.value === 'unread'
                ? 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'
                : 'bg-[var(--color-border-default)] text-[var(--color-text-secondary)]'
        const ring = isActive ? 'ring-2 ring-offset-1 ring-[var(--color-border-default)] ring-offset-[var(--color-bg-base)]' : ''
        return (
          <button key={t.value} onClick={() => handleTab(t.value)} className={`${base} ${color} ${ring}`}>
            {t.label}
            {t.value === 'action' && actionCount > 0 && (
              <span className="ml-1 opacity-70">{actionCount}</span>
            )}
          </button>
        )
      })}

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
            className="absolute right-0 top-full z-50 mt-1 w-56 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-3 text-xs shadow-lg"
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
  const auth = useAtomValue(authAtom)
  const myEmail = auth?.address ?? ''
  const [conversations, setConversations] = useAtom(conversationsAtom)
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

  // single-thread context menu action with optimistic updates
  const handleContextAction = useCallback(async (threadId: string, action: SingleAction) => {
    // save snapshot for rollback
    const snapshot = conversations

    // optimistic update for all actions
    const optimistic: Record<string, (c: ConversationSummary) => ConversationSummary> = {
      pin: (c) => ({ ...c, pinned: true }),
      unpin: (c) => ({ ...c, pinned: false }),
      archive: (c) => ({ ...c, archived: true }),
      unarchive: (c) => ({ ...c, archived: false }),
      star: (c) => ({ ...c, flagged: true }),
      unstar: (c) => ({ ...c, flagged: false }),
      read: (c) => ({ ...c, unread_count: 0 }),
      unread: (c) => ({ ...c, unread_count: Math.max(1, c.unread_count) }),
    }
    if (optimistic[action]) {
      setConversations((prev) => prev.map((c) => c.thread_id === threadId ? optimistic[action](c) : c))
    }

    try {
      if (action === 'snooze') {
        const tomorrow = new Date()
        tomorrow.setDate(tomorrow.getDate() + 1)
        tomorrow.setHours(9, 0, 0, 0)
        await snoozeConversation(threadId, tomorrow.toISOString())
        setConversations((prev) => prev.filter((c) => c.thread_id !== threadId))
        toast.success('Snoozed until tomorrow 9:00')
      } else if (action === 'delete') {
        await postJson<BatchResult>('/conversations/batch', { thread_ids: [threadId], action })
        setConversations((prev) => prev.filter((c) => c.thread_id !== threadId))
        toast.success('Deleted')
      } else if (action === 'pin' || action === 'unpin' || action === 'archive' || action === 'unarchive') {
        await postJson<ApiResult>(`/conversations/${encodeURIComponent(threadId)}/${action}`, {})
        const labels: Record<string, string> = { pin: 'Pinned', unpin: 'Unpinned', archive: 'Archived', unarchive: 'Unarchived' }
        toast.success(labels[action] ?? 'Updated')
      } else {
        await postJson<BatchResult>('/conversations/batch', { thread_ids: [threadId], action })
        toast.success('Updated')
      }
    } catch (err) {
      // rollback to snapshot on failure
      setConversations(snapshot)
      toast.error(err instanceof Error ? err.message : 'Action failed')
    }
  }, [conversations, setConversations])

  const sortOrder = useAtomValue(sortOrderAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const quickFilter = useAtomValue(quickFilterAtom)
  const folder = useAtomValue(folderAtom)

  // apply client-side filtering + sort
  const sortedConversations = useMemo(() => {
    let visible = showArchived ? conversations : conversations.filter((c) => !c.archived)

    // hide sent-only threads from inbox view (show them only in Sent tab)
    if (folder !== 'Sent' && myEmail) {
      visible = visible.filter((c) => {
        const firstEmail = extractEmail(c.participants[0] ?? '')
        return firstEmail !== myEmail
      })
    }

    // quick filter
    if (quickFilter === 'unread') {
      visible = visible.filter((c) => c.unread_count > 0)
    } else if (quickFilter === 'starred') {
      visible = visible.filter((c) => c.flagged)
    }
    // attachment filter skipped: ConversationSummary does not have has_attachments yet

    // importance section filter
    if (importanceSection === 'action') {
      visible = visible.filter((c) => c.requires_action)
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
  }, [conversations, sortOrder, showArchived, importanceSection, quickFilter, folder, myEmail])

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
      <div className="flex items-center gap-2 border-b border-[var(--color-border-default)] px-3 py-2">
        <div role="search" className="relative flex-1">
          <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--color-text-tertiary)]" aria-hidden="true" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            aria-label="Search conversations"
            className="w-full rounded-md border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] py-2 pl-9 pr-8 text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-border-strong)]"
          />
          {isSearching && (
            <button
              onClick={() => setSearchQuery('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-0.5 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
              aria-label="Clear search"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
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

        {conversations.some((c) => c.unread_count > 0) && (
          <button
            onClick={async () => {
              const unreadIds = conversations.filter((c) => c.unread_count > 0).map((c) => c.thread_id)
              if (unreadIds.length === 0) return
              try {
                await postJson('/conversations/batch', { thread_ids: unreadIds, action: 'read' })
                setConversations((prev) => prev.map((c) => ({ ...c, unread_count: 0 })))
                toast.success(`Marked ${unreadIds.length} as read`)
              } catch { toast.error('Failed') }
            }}
            aria-label="Mark all as read"
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-all duration-150 hover:bg-[var(--color-hover)]"
            title="Mark all as read"
          >
            <MailCheck className="h-5 w-5" aria-hidden="true" />
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
                    myEmail={myEmail}
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
