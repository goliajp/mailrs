import type { ContextMenuItem } from '@/components/context-menu'
import type { CategoryCount, ConversationSummary } from '@/lib/types'

import { toast } from '@goliapkg/gds'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import {
  Check,
  CheckCircle,
  Mail,
  MailCheck,
  Pin,
  Search,
  SlidersHorizontal,
  SquarePen,
  Star,
  X,
} from 'lucide-react'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { CategoryBadge, ImportanceBadge } from '@/components/category-badge'
import { ActionSheet, ContextMenu, useContextMenu } from '@/components/context-menu'
import { SenderAvatar } from '@/components/sender-avatar'
import { SwipeableRow } from '@/components/swipeable-row'
import { fetchJson, postJson, snoozeConversation } from '@/lib/api'
import { extractEmail, extractName } from '@/lib/avatar'
import { dateGroupLabel, formatDate, formatFullDate } from '@/lib/format'
import { authAtom } from '@/store/auth'
import {
  batchModeAtom,
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
  folderAtom,
  hasMoreAtom,
  type ImportanceSection,
  importanceSectionAtom,
  initialLoadingAtom,
  loadingMoreAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  selectedThreadIdsAtom,
  showArchivedAtom,
  type SortOrder,
  sortOrderAtom,
  visibleConversationIdsAtom,
} from '@/store/chat'

type ApiResult = {
  message?: string
  success: boolean
}
type BatchAction = 'archive' | 'delete' | 'read' | 'star' | 'unarchive' | 'unread' | 'unstar'

type BatchResult = {
  failed: number
  message?: string
  processed: number
  success: boolean
}

type SingleAction = 'pin' | 'snooze' | 'unpin' | BatchAction

const ConversationItem = memo(function ConversationItem({
  batchMode,
  checked,
  convo,
  myEmail,
  onContextAction,
  onSelect,
  onToggleCheck,
  selected,
}: {
  batchMode: boolean
  checked: boolean
  convo: ConversationSummary
  myEmail: string
  onContextAction: (threadId: string, action: SingleAction) => void
  onSelect: (threadId: string) => void
  onToggleCheck: (threadId: string) => void
  selected: boolean
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
  const [hovered, setHovered] = useState(false)

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
      danger: true,
      label: 'Delete',
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
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onTouchEnd={ctx.onTouchEnd}
      onTouchMove={ctx.onTouchMove}
      onTouchStart={ctx.onTouchStart}
      role="listitem"
    >
      <button
        aria-label={`${name}: ${convo.subject || '(no subject)'}${hasUnread ? `, ${convo.unread_count} unread` : ''}${isPinned ? ', pinned' : ''}`}
        aria-selected={selected && !batchMode}
        className={`focus-visible:ring-accent/50 relative flex w-full items-start gap-3 border-l-[3px] px-4 py-2.5 text-left transition-all duration-150 focus-visible:ring-2 focus-visible:outline-none ${
          selected && !batchMode
            ? 'border-l-accent'
            : hasUnread
              ? 'border-l-accent'
              : 'border-l-transparent'
        } ${!hasUnread && !selected && !checked ? 'opacity-70 hover:opacity-100' : ''} ${
          selected && !batchMode
            ? 'bg-accent/10'
            : checked
              ? 'bg-accent/10'
              : 'hover:bg-bg-secondary'
        }`}
        onClick={handleClick}
        onContextMenu={ctx.open}
      >
        {batchMode && (
          <div className="mt-0.5 flex shrink-0 items-center">
            <div
              className={`flex h-5 w-5 items-center justify-center rounded border-2 transition-colors ${
                checked ? 'border-accent bg-accent' : 'border-border bg-bg'
              }`}
            >
              {checked && <Check className="h-3 w-3 text-white" />}
            </div>
          </div>
        )}
        <SenderAvatar sender={firstParticipant} size={36} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <span
              className={`truncate text-sm ${isOwn ? 'text-accent' : ''} ${hasUnread ? 'text-fg font-semibold' : isOwn ? '' : 'text-fg-secondary'}`}
            >
              {name}
              {convo.participants.length > 1 && (
                <span className="text-fg-muted"> +{convo.participants.length - 1}</span>
              )}
            </span>
            <div className="flex shrink-0 items-center gap-1.5">
              {convo.message_count > 1 && (
                <span className="bg-bg-secondary text-fg-muted rounded px-1 py-px text-xs tabular-nums md:text-[10px]">
                  {convo.message_count}
                </span>
              )}
              {isPinned && <Pin className="text-accent h-3 w-3" />}
              {/* mobile: always show action buttons; desktop: show on hover */}
              {!batchMode && (
                <span className={`flex items-center gap-0.5 ${hovered ? '' : 'hidden'} md:hidden`}>
                  <button
                    className="touch-target text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary rounded p-1"
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isArchived ? 'unarchive' : 'archive')
                    }}
                    title={isArchived ? 'Unarchive' : 'Archive'}
                  >
                    <Mail className="h-4 w-4" />
                  </button>
                  <button
                    className={`touch-target rounded p-1 ${isFlagged ? 'text-warning' : 'text-fg-muted hover:text-fg-secondary'}`}
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isFlagged ? 'unstar' : 'star')
                    }}
                    title={isFlagged ? 'Unstar' : 'Star'}
                  >
                    <Star className="h-4 w-4" fill={isFlagged ? 'currentColor' : 'none'} />
                  </button>
                </span>
              )}
              {/* desktop: hover actions */}
              {hovered && !batchMode ? (
                <span className="hidden items-center gap-0.5 md:flex">
                  <button
                    className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary rounded p-0.5"
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isArchived ? 'unarchive' : 'archive')
                    }}
                    title={isArchived ? 'Unarchive' : 'Archive'}
                  >
                    <Mail className="h-3.5 w-3.5" />
                  </button>
                  <button
                    className={`hover:bg-bg-secondary rounded p-0.5 ${isFlagged ? 'text-warning' : 'text-fg-muted hover:text-fg-secondary'}`}
                    onClick={(e) => {
                      e.stopPropagation()
                      onContextAction(convo.thread_id, isFlagged ? 'unstar' : 'star')
                    }}
                    title={isFlagged ? 'Unstar' : 'Star'}
                  >
                    <Star className="h-3.5 w-3.5" fill={isFlagged ? 'currentColor' : 'none'} />
                  </button>
                </span>
              ) : (
                <span
                  className="text-fg-muted hidden text-xs md:inline"
                  title={formatFullDate(convo.last_date)}
                >
                  {formatDate(convo.last_date)}
                </span>
              )}
              {/* mobile: always show timestamp */}
              <span
                className="text-fg-muted text-xs md:hidden"
                title={formatFullDate(convo.last_date)}
              >
                {formatDate(convo.last_date)}
              </span>
            </div>
          </div>
          <div className="flex items-center gap-1.5">
            <p
              className={`min-w-0 flex-1 truncate text-sm ${hasUnread ? 'text-fg font-medium' : 'text-fg-muted'}`}
            >
              {convo.subject || '(no subject)'}
            </p>
            {isFlagged && (
              <Star className="text-warning h-3.5 w-3.5 shrink-0" fill="currentColor" />
            )}
            <span className="shrink-0">
              <ImportanceBadge level={convo.importance_level} />
            </span>
            {convo.category && convo.category !== 'general' && (
              <span className="shrink-0">
                <CategoryBadge category={convo.category} />
              </span>
            )}
            {hasUnread && (
              <span className="bg-accent flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full px-1.5 text-xs font-medium text-white">
                {convo.unread_count}
              </span>
            )}
          </div>
          {convo.snippet && <p className="text-fg-muted truncate text-xs">{convo.snippet}</p>}
        </div>
      </button>
      <ContextMenu items={contextItems} onClose={ctx.close} position={ctx.position} />
      <ActionSheet items={contextItems} onClose={ctx.close} open={ctx.actionSheetOpen} />
    </div>
  )
})

// unified tab bar
const VIEW_TABS: { label: string; value: string }[] = [
  { label: 'All', value: 'all' },
  { label: 'Unread', value: 'unread' },
  { label: 'Starred', value: 'starred' },
  { label: 'Sent', value: 'sent' },
  { label: 'Action', value: 'action' },
  { label: 'Spam', value: 'spam' },
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
    const domainsParam =
      selectedDomainsVal.length > 0
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
  const activeTab =
    activeCategory === 'spam' || activeCategory === 'scam'
      ? 'spam'
      : folder === 'Sent'
        ? 'sent'
        : section === 'action'
          ? 'action'
          : quickFilter !== 'all'
            ? quickFilter
            : 'all'

  // chips behave like Gmail tabs: clicking the active one is a no-op,
  // users clear filters by clicking All
  const handleTab = (tab: string) => {
    if (tab === activeTab) return
    setActiveCategory(null)
    setFolder(null)
    setQuickFilter('all')
    setSection(null)
    if (tab === 'spam') {
      setActiveCategory('spam')
    } else if (tab === 'sent') {
      setFolder('Sent')
    } else if (tab === 'action') {
      setSection('action')
    } else if (tab === 'unread') {
      setQuickFilter('unread')
    } else if (tab === 'starred') {
      setQuickFilter('starred')
    }
    // tab === 'all' → all filters cleared above
  }

  // action count for badge
  const [actionCount, setActionCount] = useState(0)
  useEffect(() => {
    const doms =
      selectedDomainsVal.length > 0
        ? `?domains=${encodeURIComponent(selectedDomainsVal.join(','))}`
        : ''
    fetchJson<{ count: number }>(`/conversations/action-count${doms}`).then(
      (d) => setActionCount(d.count),
      () => {}
    )
  }, [selectedDomainsVal, conversations])

  // whether any advanced filters are active
  const hasAdvancedFilters =
    sortOrder !== 'newest' ||
    showArchived ||
    (activeCategory !== null && activeCategory !== 'spam' && activeCategory !== 'scam') ||
    selectedDomains.length > 0 ||
    section === 'important' ||
    section === 'other'

  return (
    <div className="border-border flex items-center gap-1 border-b px-3 py-1.5">
      {/* main tabs — horizontally scrollable on mobile */}
      <div className="scrollbar-hide flex snap-x snap-mandatory items-center gap-1 overflow-x-auto scroll-smooth md:overflow-x-visible">
        {VIEW_TABS.map((t) => {
          const isActive = activeTab === t.value
          const base =
            'snap-start shrink-0 rounded-md px-3 py-1 text-xs font-medium transition-colors cursor-pointer'
          const color =
            t.value === 'spam'
              ? 'bg-danger/10 text-danger'
              : t.value === 'action'
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
              {t.value === 'action' && actionCount > 0 && (
                <span className="ml-1 opacity-70">{actionCount}</span>
              )}
            </button>
          )
        })}
      </div>

      {/* filter dropdown toggle */}
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

        {/* filter dropdown panel */}
        {filtersOpen && (
          <div className="border-border bg-surface absolute top-full right-0 z-50 mt-1 w-56 rounded-lg border p-3 text-xs shadow-lg">
            {/* sort */}
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

            {/* view: active / archived */}
            <div className="mb-3">
              <label className="text-fg-muted mb-1 block font-medium">View</label>
              <div className="flex gap-1">
                <button
                  className={`rounded-md px-2 py-0.5 transition-colors ${
                    !showArchived ? 'bg-fg text-bg' : 'text-fg-secondary hover:bg-bg-secondary'
                  }`}
                  onClick={() => setShowArchived(false)}
                >
                  Active
                </button>
                <button
                  className={`rounded-md px-2 py-0.5 transition-colors ${
                    showArchived ? 'bg-fg text-bg' : 'text-fg-secondary hover:bg-bg-secondary'
                  }`}
                  onClick={() => setShowArchived(true)}
                >
                  Archived
                </button>
              </div>
            </div>

            {/* priority */}
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

            {/* categories */}
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

            {/* reset all filters */}
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
}

const dateLabel = dateGroupLabel

type VirtualListItem =
  | { convo: ConversationSummary; type: 'conversation' }
  | { label: string; type: 'divider' }
  | { type: 'end' }
  | { type: 'sentinel' }

// module-level: survives component unmount/remount on mobile view switching
let savedScrollTop = 0

export function ConversationList({
  onLoadMore,
  onRefresh,
  onSelectConversation,
}: {
  onLoadMore: () => void
  onRefresh?: () => Promise<void> | void
  onSelectConversation?: () => void
}) {
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

  // save scroll position when leaving list, restore when coming back
  useEffect(() => {
    const el = scrollContainerRef.current
    if (el && savedScrollTop > 0) {
      el.scrollTop = savedScrollTop
      savedScrollTop = 0
    }
  }, [])

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
  const toggleThreadCheck = useCallback(
    (threadId: string) => {
      setSelectedThreadIds((prev) => {
        const next = new Set(prev)
        if (next.has(threadId)) {
          next.delete(threadId)
        } else {
          next.add(threadId)
        }
        return next
      })
    },
    [setSelectedThreadIds]
  )

  // execute batch action against API then refresh
  const handleBatchAction = useCallback(
    async (action: BatchAction) => {
      const ids = Array.from(selectedThreadIds)
      if (ids.length === 0) return

      setBatchLoading(true)
      try {
        const result = await postJson<BatchResult>('/conversations/batch', {
          action,
          thread_ids: ids,
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
    },
    [selectedThreadIds, exitBatchMode]
  )

  // single-thread context menu action with optimistic updates
  const handleContextAction = useCallback(
    async (threadId: string, action: SingleAction) => {
      // save snapshot for rollback
      const snapshot = conversations

      // optimistic update for all actions
      const optimistic: Record<string, (c: ConversationSummary) => ConversationSummary> = {
        archive: (c) => ({ ...c, archived: true }),
        pin: (c) => ({ ...c, pinned: true }),
        read: (c) => ({ ...c, unread_count: 0 }),
        star: (c) => ({ ...c, flagged: true }),
        unarchive: (c) => ({ ...c, archived: false }),
        unpin: (c) => ({ ...c, pinned: false }),
        unread: (c) => ({ ...c, unread_count: Math.max(1, c.unread_count) }),
        unstar: (c) => ({ ...c, flagged: false }),
      }
      if (optimistic[action]) {
        setConversations((prev) =>
          prev.map((c) => (c.thread_id === threadId ? optimistic[action](c) : c))
        )
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
          await postJson<BatchResult>('/conversations/batch', {
            action,
            thread_ids: [threadId],
          })
          setConversations((prev) => prev.filter((c) => c.thread_id !== threadId))
          toast.success('Deleted')
        } else if (
          action === 'pin' ||
          action === 'unpin' ||
          action === 'archive' ||
          action === 'unarchive'
        ) {
          await postJson<ApiResult>(`/conversations/${encodeURIComponent(threadId)}/${action}`, {})
          const labels: Record<string, string> = {
            archive: 'Archived',
            pin: 'Pinned',
            unarchive: 'Unarchived',
            unpin: 'Unpinned',
          }
          toast.success(labels[action] ?? 'Updated')
        } else {
          await postJson<BatchResult>('/conversations/batch', {
            action,
            thread_ids: [threadId],
          })
          toast.success('Updated')
        }
      } catch (err) {
        // rollback to snapshot on failure
        setConversations(snapshot)
        toast.error(err instanceof Error ? err.message : 'Action failed')
      }
    },
    [conversations, setConversations]
  )

  const sortOrder = useAtomValue(sortOrderAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const quickFilter = useAtomValue(quickFilterAtom)
  const folder = useAtomValue(folderAtom)

  // apply client-side filtering + sort
  const sortedConversations = useMemo(() => {
    let visible = showArchived ? conversations : conversations.filter((c) => !c.archived)

    // hide sent-only threads from inbox view (show them only in Sent tab)
    // a thread with replies from others should appear in both inbox and sent
    if (folder !== 'Sent' && myEmail) {
      visible = visible.filter((c) => {
        const emails = c.participants.map((p) => extractEmail(p))
        return !emails.every((e) => e === myEmail)
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
  const handleSelect = useCallback(
    (threadId: string) => {
      // save scroll position before navigating to thread
      if (scrollContainerRef.current) {
        savedScrollTop = scrollContainerRef.current.scrollTop
      }
      setSelectedId(threadId)
      setComposingNew(false)
      onSelectConversation?.()
    },
    [setSelectedId, setComposingNew, onSelectConversation]
  )

  const isSearching = searchQuery.trim().length > 0
  const hasBatchBar = batchMode && selectedThreadIds.size > 0

  return (
    <div className="relative flex h-full flex-col select-none">
      <div className="border-border flex items-center gap-2 border-b px-3 py-2">
        <div className="relative flex-1" role="search">
          <Search
            aria-hidden="true"
            className="text-fg-muted absolute top-1/2 left-2.5 h-4 w-4 -translate-y-1/2"
          />
          <input
            aria-label="Search conversations"
            className="border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:bg-bg w-full rounded-md border py-2 pr-8 pl-9 text-sm transition-colors outline-none"
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            type="text"
            value={searchQuery}
          />
          {isSearching && (
            <button
              aria-label="Clear search"
              className="text-fg-muted hover:text-fg-secondary absolute top-1/2 right-2 -translate-y-1/2 rounded p-0.5"
              onClick={() => setSearchQuery('')}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>

        {/* batch select toggle — hidden during search */}
        {!isSearching && (
          <button
            aria-label={batchMode ? 'Exit batch select mode' : 'Enter batch select mode'}
            aria-pressed={batchMode}
            className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150 ${
              batchMode ? 'bg-accent/10 text-accent' : 'text-fg-muted hover:bg-bg-secondary'
            }`}
            onClick={() => {
              if (batchMode) {
                exitBatchMode()
              } else {
                setBatchMode(true)
              }
            }}
            title="Batch select"
          >
            <CheckCircle aria-hidden="true" className="h-5 w-5" />
          </button>
        )}

        {conversations.some((c) => c.unread_count > 0) && (
          <button
            aria-label="Mark all as read"
            className="text-fg-muted hover:bg-bg-secondary flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150"
            onClick={async () => {
              const unreadIds = conversations
                .filter((c) => c.unread_count > 0)
                .map((c) => c.thread_id)
              if (unreadIds.length === 0) return
              try {
                await postJson('/conversations/batch', {
                  action: 'read',
                  thread_ids: unreadIds,
                })
                setConversations((prev) => prev.map((c) => ({ ...c, unread_count: 0 })))
                toast.success(`Marked ${unreadIds.length} as read`)
              } catch {
                toast.error('Failed')
              }
            }}
            title="Mark all as read"
          >
            <MailCheck aria-hidden="true" className="h-5 w-5" />
          </button>
        )}

        <button
          aria-label="New conversation"
          className="text-fg-muted hover:bg-bg-secondary flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150"
          onClick={() => {
            setComposingNew(true)
            setSelectedId(null)
          }}
          title="New conversation"
        >
          <SquarePen aria-hidden="true" className="h-5 w-5" />
        </button>
      </div>

      <FilterBar />

      <VirtualConversationList
        batchMode={batchMode}
        conversations={sortedConversations}
        dateLabel={dateLabel}
        folder={folder}
        hasBatchBar={hasBatchBar}
        hasMore={hasMore}
        initialLoading={initialLoading}
        isSearching={isSearching}
        loadingMore={loadingMore}
        myEmail={myEmail}
        onContextAction={handleContextAction}
        onLoadMore={sentinelCallback}
        onRefresh={onRefresh}
        onSelect={handleSelect}
        onToggleCheck={toggleThreadCheck}
        scrollContainerRef={scrollContainerRef}
        selectedId={selectedId}
        selectedThreadIds={selectedThreadIds}
        showArchived={showArchived}
      />

      {/* floating batch action bar */}
      {hasBatchBar && (
        <BatchActionBar
          loading={batchLoading}
          onAction={handleBatchAction}
          onCancel={exitBatchMode}
          selectedCount={selectedThreadIds.size}
        />
      )}
    </div>
  )
}

// floating action bar at bottom of list during batch mode
function BatchActionBar({
  loading,
  onAction,
  onCancel,
  selectedCount,
}: {
  loading: boolean
  onAction: (action: BatchAction) => void
  onCancel: () => void
  selectedCount: number
}) {
  return (
    <div className="border-border bg-surface absolute right-0 bottom-0 left-0 z-40 border-t px-3 py-2 backdrop-blur">
      <div className="flex items-center gap-2">
        <span className="text-fg-secondary shrink-0 text-xs font-medium">
          {selectedCount} selected
        </span>
        <div className="flex flex-1 items-center gap-1.5 overflow-x-auto">
          <button
            className="text-fg-secondary hover:bg-bg-secondary focus-visible:ring-accent/50 shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50"
            disabled={loading}
            onClick={() => onAction('read')}
          >
            Mark read
          </button>
          <button
            className="text-fg-secondary hover:bg-bg-secondary focus-visible:ring-accent/50 shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50"
            disabled={loading}
            onClick={() => onAction('unread')}
          >
            Mark unread
          </button>
          <button
            className="text-fg-secondary hover:bg-bg-secondary focus-visible:ring-accent/50 shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50"
            disabled={loading}
            onClick={() => onAction('star')}
          >
            Star
          </button>
          <button
            className="text-fg-secondary hover:bg-bg-secondary focus-visible:ring-accent/50 shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50"
            disabled={loading}
            onClick={() => onAction('archive')}
          >
            Archive
          </button>
          <button
            className="text-danger hover:bg-danger/10 focus-visible:ring-accent/50 shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50"
            disabled={loading}
            onClick={() => onAction('delete')}
          >
            Delete
          </button>
        </div>
        <button
          className="text-fg-muted hover:bg-bg-secondary shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors disabled:opacity-50"
          disabled={loading}
          onClick={onCancel}
        >
          Cancel
        </button>
        {loading && (
          <div className="border-border border-t-fg-secondary h-4 w-4 shrink-0 animate-spin rounded-full border-2" />
        )}
      </div>
    </div>
  )
}

function ConversationSkeleton() {
  return (
    <div className="animate-pulse">
      {Array.from({ length: 8 }).map((_, i) => (
        <div className="flex items-start gap-3 px-4 py-3" key={i}>
          <div className="bg-border h-9 w-9 shrink-0 rounded-full" />
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex items-center justify-between">
              <div className="bg-border h-3.5 w-24 rounded" />
              <div className="bg-border h-3 w-10 rounded" />
            </div>
            <div className="bg-border h-3 w-40 rounded" />
          </div>
        </div>
      ))}
    </div>
  )
}

function DateDivider({ label }: { label: string }) {
  return (
    <div className="sticky top-0 z-10 flex justify-center py-1.5 select-none">
      <span className="bg-bg-secondary text-fg-muted rounded-full px-2.5 py-0.5 text-xs font-medium md:text-[10px]">
        {label}
      </span>
    </div>
  )
}

function VirtualConversationList({
  batchMode,
  conversations,
  dateLabel,
  folder,
  hasBatchBar,
  hasMore,
  initialLoading,
  isSearching,
  loadingMore,
  myEmail,
  onContextAction,
  onLoadMore,
  onRefresh,
  onSelect,
  onToggleCheck,
  scrollContainerRef,
  selectedId,
  selectedThreadIds,
  showArchived,
}: {
  batchMode: boolean
  conversations: ConversationSummary[]
  dateLabel: (ts: number) => string
  folder: null | string
  hasBatchBar: boolean
  hasMore: boolean
  initialLoading: boolean
  isSearching: boolean
  loadingMore: boolean
  myEmail: string
  onContextAction: (threadId: string, action: SingleAction) => void
  onLoadMore: (node: HTMLDivElement | null) => void
  onRefresh?: () => Promise<void> | void
  onSelect: (threadId: string) => void
  onToggleCheck: (threadId: string) => void
  scrollContainerRef: React.RefObject<HTMLDivElement | null>
  selectedId: null | string
  selectedThreadIds: Set<string>
  showArchived: boolean
}) {
  // build flat list of items
  const items = useMemo<VirtualListItem[]>(() => {
    if (conversations.length === 0) return []
    const result: VirtualListItem[] = []
    let prevGroup = ''
    for (const c of conversations) {
      const group = dateLabel(c.last_date)
      if (group !== prevGroup) {
        result.push({ label: group, type: 'divider' })
        prevGroup = group
      }
      result.push({ convo: c, type: 'conversation' })
    }
    if (hasMore) result.push({ type: 'sentinel' })
    else result.push({ type: 'end' })
    return result
  }, [conversations, dateLabel, hasMore])

  const parentRef = useRef<HTMLDivElement>(null)

  const virtualizer = useVirtualizer({
    count: items.length,
    overscan: 10,
    estimateSize: (index) => {
      const item = items[index]
      if (item.type === 'divider') return 32
      if (item.type === 'sentinel' || item.type === 'end') return 48
      return 72
    },
    getScrollElement: () => parentRef.current,
  })

  // pull-to-refresh state (must be before early returns)
  const [pullDistance, setPullDistance] = useState(0)
  const [refreshing, setRefreshing] = useState(false)
  const pullStartY = useRef(0)
  const isPulling = useRef(false)

  const handlePullStart = useCallback(
    (e: React.TouchEvent) => {
      if (!onRefresh || !parentRef.current || parentRef.current.scrollTop > 0) return
      pullStartY.current = e.touches[0].clientY
      isPulling.current = true
    },
    [onRefresh]
  )

  const handlePullMove = useCallback(
    (e: React.TouchEvent) => {
      if (!isPulling.current || refreshing) return
      const dy = e.touches[0].clientY - pullStartY.current
      if (dy > 0) {
        setPullDistance(Math.min(80, dy * 0.4))
      } else {
        isPulling.current = false
        setPullDistance(0)
      }
    },
    [refreshing]
  )

  const handlePullEnd = useCallback(async () => {
    if (!isPulling.current || !onRefresh) return
    isPulling.current = false
    if (pullDistance >= 60) {
      setRefreshing(true)
      try {
        await onRefresh()
      } finally {
        setRefreshing(false)
      }
    }
    setPullDistance(0)
  }, [pullDistance, onRefresh])

  if (initialLoading && conversations.length === 0) {
    return (
      <div
        className={`flex-1 overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
        ref={scrollContainerRef}
        role="list"
      >
        <ConversationSkeleton />
      </div>
    )
  }

  if (conversations.length === 0) {
    return (
      <div
        className={`flex-1 overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
        ref={scrollContainerRef}
        role="list"
      >
        <div className="text-fg-muted flex flex-col items-center justify-center p-8 text-center">
          <Mail aria-hidden="true" className="text-fg-muted mb-3 h-10 w-10" strokeWidth={1} />
          <p className="text-sm font-medium">
            {isSearching
              ? 'No results found'
              : folder === 'Sent'
                ? 'No sent messages'
                : folder === 'Drafts'
                  ? 'No drafts'
                  : folder === 'Trash'
                    ? 'Trash is empty'
                    : showArchived
                      ? 'No archived conversations'
                      : 'All caught up!'}
          </p>
          <p className="mt-1 text-xs">{isSearching ? 'Try a different search term' : ''}</p>
        </div>
      </div>
    )
  }

  return (
    <div
      aria-label="Conversations"
      className={`flex-1 overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
      onTouchEnd={handlePullEnd}
      onTouchMove={handlePullMove}
      onTouchStart={handlePullStart}
      ref={(node) => {
        // share ref between virtualizer and external scroll container
        ;(parentRef as React.MutableRefObject<HTMLDivElement | null>).current = node
        if (scrollContainerRef && 'current' in scrollContainerRef) {
          ;(scrollContainerRef as React.MutableRefObject<HTMLDivElement | null>).current = node
        }
      }}
      role="list"
    >
      {/* pull-to-refresh indicator */}
      {(pullDistance > 0 || refreshing) && (
        <div
          className="flex items-center justify-center md:hidden"
          style={{ height: refreshing ? 40 : pullDistance }}
        >
          <div
            className={`border-border border-t-accent h-5 w-5 rounded-full border-2 ${refreshing ? 'animate-spin' : ''}`}
            style={refreshing ? undefined : { transform: `rotate(${pullDistance * 4}deg)` }}
          />
        </div>
      )}
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((virtualItem) => {
          const item = items[virtualItem.index]
          return (
            <div
              className="absolute top-0 left-0 w-full"
              data-index={virtualItem.index}
              key={virtualItem.index}
              ref={virtualizer.measureElement}
              style={{ transform: `translateY(${virtualItem.start}px)` }}
            >
              {item.type === 'divider' && <DateDivider label={item.label} />}
              {item.type === 'conversation' && (
                <SwipeableRow
                  onSwipeLeft={() => onContextAction(item.convo.thread_id, 'delete')}
                  onSwipeRight={() =>
                    onContextAction(
                      item.convo.thread_id,
                      item.convo.archived ? 'unarchive' : 'archive'
                    )
                  }
                >
                  <ConversationItem
                    batchMode={batchMode}
                    checked={selectedThreadIds.has(item.convo.thread_id)}
                    convo={item.convo}
                    myEmail={myEmail}
                    onContextAction={onContextAction}
                    onSelect={onSelect}
                    onToggleCheck={onToggleCheck}
                    selected={selectedId === item.convo.thread_id}
                  />
                </SwipeableRow>
              )}
              {item.type === 'sentinel' && (
                <div className="flex justify-center py-4" ref={onLoadMore}>
                  {loadingMore && (
                    <div className="border-border border-t-fg-secondary h-5 w-5 animate-spin rounded-full border-2" />
                  )}
                </div>
              )}
              {item.type === 'end' && (
                <div className="text-fg-muted py-3 text-center text-xs">No more conversations</div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
