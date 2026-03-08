import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { Archive, Check, CheckCircle, Inbox, Mail, MailOpen, Paperclip, Pin, Search, SquarePen, Star } from 'lucide-react'
import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { toast } from 'sonner'

import { CategoryBadge, ImportanceBadge } from '@/components/category-badge'
import { ContextMenu, useContextMenu } from '@/components/context-menu'
import type { ContextMenuItem } from '@/components/context-menu'
import { fetchJson, postJson } from '@/lib/api'
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
  showArchivedAtom,
  sortOrderAtom,
  visibleConversationIdsAtom,
  quickFilterAtom,
  type ImportanceSection,
  type QuickFilter,
  type SortOrder,
} from '@/store/chat'

type BatchAction = 'read' | 'unread' | 'delete' | 'star' | 'unstar' | 'archive' | 'unarchive'
type SingleAction = BatchAction | 'pin' | 'unpin'

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

function QuickBtn({ onClick, title, children }: { onClick: (e: React.MouseEvent) => void; title: string; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      title={title}
      className="rounded-md p-1 text-zinc-400 hover:bg-zinc-200 hover:text-zinc-600 dark:hover:bg-zinc-700 dark:hover:text-zinc-300"
    >
      {children}
    </button>
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
    <div role="listitem">
    <button
      onClick={handleClick}
      onContextMenu={ctx.open}
      aria-selected={selected && !batchMode}
      aria-label={`${name}: ${convo.subject || '(no subject)'}${hasUnread ? `, ${convo.unread_count} unread` : ''}${isPinned ? ', pinned' : ''}`}
      className={`group/item relative flex w-full items-start gap-3 px-4 py-3 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 ${
        hasUnread ? 'border-l-[3px] border-l-blue-600' : 'border-l-[3px] border-l-transparent'
      } ${
        !hasUnread && !selected && !checked ? 'opacity-70 hover:opacity-100' : ''
      } ${
        selected && !batchMode
          ? 'bg-zinc-100 dark:bg-zinc-800'
          : checked
            ? 'bg-blue-50 dark:bg-blue-900/20'
            : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
      }`}
    >
      {batchMode && (
        <div className="mt-0.5 flex shrink-0 items-center">
          <div
            className={`flex h-5 w-5 items-center justify-center rounded border-2 transition-colors ${
              checked
                ? 'border-blue-600 bg-blue-600'
                : 'border-zinc-300 bg-white dark:border-zinc-600 dark:bg-zinc-900'
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
            className={`select-text truncate text-sm ${hasUnread ? 'font-semibold text-zinc-900 dark:text-zinc-100' : 'text-zinc-700 dark:text-zinc-300'}`}
          >
            {name}
            {convo.participants.length > 1 && (
              <span className="text-zinc-400">
                {' '}
                +{convo.participants.length - 1}
              </span>
            )}
          </span>
          <div className="flex shrink-0 items-center gap-1">
            {isPinned && (
              <Pin className="h-3 w-3 text-blue-600 dark:text-blue-400" />
            )}
            <span className="text-xs text-zinc-400" title={formatFullDate(convo.last_date)}>
              {formatDate(convo.last_date)}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <p
            className={`min-w-0 flex-1 select-text truncate text-sm ${hasUnread ? 'font-medium text-zinc-800 dark:text-zinc-200' : 'text-zinc-500 dark:text-zinc-400'}`}
          >
            {convo.subject || '(no subject)'}
          </p>
          {isFlagged && (
            <Star className="h-3.5 w-3.5 shrink-0 text-yellow-400" fill="currentColor" />
          )}
          <ImportanceBadge level={convo.importance_level} />
          {convo.category && convo.category !== 'general' && (
            <CategoryBadge category={convo.category} />
          )}
          {hasUnread && (
            <span className="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-blue-600 px-1.5 text-xs font-medium text-white">
              {convo.unread_count}
            </span>
          )}
        </div>
        {convo.snippet && (
          <p className="select-text truncate text-xs text-zinc-400 dark:text-zinc-500">
            {convo.snippet}
          </p>
        )}
      </div>
      {/* hover quick actions */}
      {!batchMode && (
        <div className="absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-0.5 rounded-md bg-white/90 px-1 py-0.5 opacity-0 backdrop-blur-sm transition-opacity group-hover/item:opacity-100 dark:bg-zinc-900/90">
          <QuickBtn onClick={(e) => { e.stopPropagation(); onContextAction(convo.thread_id, hasUnread ? 'read' : 'unread') }} title={hasUnread ? 'Mark read' : 'Mark unread'}>
            {hasUnread ? <MailOpen className="h-3.5 w-3.5" /> : <Mail className="h-3.5 w-3.5" />}
          </QuickBtn>
          <QuickBtn onClick={(e) => { e.stopPropagation(); onContextAction(convo.thread_id, isArchived ? 'unarchive' : 'archive') }} title={isArchived ? 'Unarchive' : 'Archive'}>
            <Archive className="h-3.5 w-3.5" />
          </QuickBtn>
          <QuickBtn onClick={(e) => { e.stopPropagation(); onContextAction(convo.thread_id, isFlagged ? 'unstar' : 'star') }} title={isFlagged ? 'Unstar' : 'Star'}>
            <Star className="h-3.5 w-3.5" fill={isFlagged ? 'currentColor' : 'none'} />
          </QuickBtn>
        </div>
      )}
    </button>
    <ContextMenu position={ctx.position} items={contextItems} onClose={ctx.close} />
    </div>
  )
})

const QUICK_FILTERS: { value: QuickFilter; label: string; icon: React.ReactNode }[] = [
  { value: 'all', label: 'All', icon: <Inbox className="h-3.5 w-3.5" /> },
  { value: 'unread', label: 'Unread', icon: <Mail className="h-3.5 w-3.5" /> },
  { value: 'starred', label: 'Starred', icon: <Star className="h-3.5 w-3.5" /> },
  { value: 'attachment', label: 'Files', icon: <Paperclip className="h-3.5 w-3.5" /> },
]

function QuickFilterBar() {
  const [filter, setFilter] = useAtom(quickFilterAtom)
  return (
    <div className="flex select-none gap-1 border-b border-zinc-200 px-3 py-1.5 dark:border-zinc-800">
      {QUICK_FILTERS.map((f) => (
        <button
          key={f.value}
          onClick={() => setFilter(f.value)}
          aria-pressed={filter === f.value}
          className={`flex items-center gap-1 rounded-md px-2 py-0.5 text-xs font-medium transition-colors ${
            filter === f.value
              ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
              : 'text-zinc-500 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800'
          }`}
        >
          {f.icon}
          {f.label}
        </button>
      ))}
    </div>
  )
}

function DomainSelector() {
  const auth = useAtomValue(authAtom)
  const [selectedDomains, setSelectedDomains] = useAtom(selectedDomainsAtom)
  const [crossAccountRead, setCrossAccountRead] = useAtom(crossAccountReadAtom)

  const superDomains = auth?.super_domains ?? []
  if (superDomains.length === 0) return null

  const toggleDomain = (domain: string) => {
    setSelectedDomains((prev) =>
      prev.includes(domain)
        ? prev.filter((d) => d !== domain)
        : [...prev, domain]
    )
  }

  const allSelected = selectedDomains.length === superDomains.length
  const toggleAll = () => {
    if (allSelected) {
      setSelectedDomains([])
    } else {
      setSelectedDomains([...superDomains])
    }
  }

  return (
    <div className="flex items-center gap-1.5 overflow-x-auto border-b border-zinc-200 px-3 py-1.5 dark:border-zinc-800" role="group" aria-label="Domain filter">
      <span className="shrink-0 text-xs text-zinc-400" aria-hidden="true">Domains:</span>
      <button
        onClick={toggleAll}
        aria-pressed={selectedDomains.length === 0}
        aria-label={selectedDomains.length === 0 ? 'Show my emails only' : allSelected ? 'Show all domains' : 'Mixed domain selection'}
        className={`shrink-0 rounded px-2 py-0.5 text-xs font-medium transition-colors ${
          selectedDomains.length === 0
            ? 'bg-blue-600 text-white'
            : 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
        }`}
      >
        {selectedDomains.length === 0 ? 'Mine' : allSelected ? 'All' : 'Mix'}
      </button>
      {superDomains.map((domain) => (
        <button
          key={domain}
          onClick={() => toggleDomain(domain)}
          aria-pressed={selectedDomains.includes(domain)}
          aria-label={`Filter by domain ${domain}`}
          className={`shrink-0 rounded px-2 py-0.5 text-xs font-medium transition-colors ${
            selectedDomains.includes(domain)
              ? 'bg-blue-600 text-white'
              : 'bg-zinc-100 text-zinc-500 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
          }`}
        >
          {domain}
        </button>
      ))}
      {selectedDomains.length > 0 && (
        <label className="ml-auto flex shrink-0 cursor-pointer items-center gap-1 text-xs text-zinc-500">
          <input
            type="checkbox"
            checked={crossAccountRead}
            onChange={(e) => setCrossAccountRead(e.target.checked)}
            className="h-3 w-3 rounded border-zinc-300 accent-blue-600"
          />
          Cross-read
        </label>
      )}
    </div>
  )
}

function CategoryChips() {
  const [categories, setCategories] = useState<CategoryCount[]>([])
  const [activeCategory, setActiveCategory] = useAtom(categoryFilterAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)

  useEffect(() => {
    const domainsParam = selectedDomains.length > 0
      ? `?domains=${encodeURIComponent(selectedDomains.join(','))}`
      : ''
    fetchJson<CategoryCount[]>(`/conversations/categories${domainsParam}`).then(
      (data) => setCategories(data),
      () => {}
    )
  }, [selectedDomains])

  if (categories.length === 0) return null

  return (
    <div className="flex gap-1.5 overflow-x-auto border-b border-zinc-200 px-3 py-1.5 dark:border-zinc-800" role="group" aria-label="Category filter">
      <button
        onClick={() => setActiveCategory(null)}
        aria-pressed={activeCategory === null}
        aria-label="Show all categories"
        className={`shrink-0 rounded-md px-2.5 py-0.5 text-xs font-medium transition-colors ${
          activeCategory === null
            ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
            : 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
        }`}
      >
        All
      </button>
      {categories.map((cat) => (
        <button
          key={cat.category}
          onClick={() =>
            setActiveCategory(
              activeCategory === cat.category ? null : cat.category
            )
          }
          aria-pressed={activeCategory === cat.category}
          aria-label={`Filter by category ${cat.category}, ${cat.count} conversations`}
          className={`shrink-0 rounded-md px-2.5 py-0.5 text-xs font-medium capitalize transition-colors ${
            activeCategory === cat.category
              ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
              : 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
          }`}
        >
          {cat.category} ({cat.count})
        </button>
      ))}
    </div>
  )
}

const SORT_OPTIONS: { value: SortOrder; label: string }[] = [
  { value: 'newest', label: 'Newest' },
  { value: 'oldest', label: 'Oldest' },
  { value: 'unread', label: 'Unread first' },
]

function SortAndArchivedRow() {
  const [sortOrder, setSortOrder] = useAtom(sortOrderAtom)
  const [showArchived, setShowArchived] = useAtom(showArchivedAtom)

  return (
    <div className="flex items-center gap-1 overflow-hidden border-b border-zinc-200 px-3 py-1.5 dark:border-zinc-800" role="group" aria-label="Sort order">
      {SORT_OPTIONS.map((opt) => (
        <button
          key={opt.value}
          onClick={() => setSortOrder(opt.value)}
          aria-pressed={sortOrder === opt.value}
          aria-label={`Sort by ${opt.label}`}
          className={`truncate rounded-md px-2 py-0.5 text-xs font-medium transition-colors ${
            sortOrder === opt.value
              ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
              : 'text-zinc-500 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800'
          }`}
        >
          {opt.label}
        </button>
      ))}
      <button
        onClick={() => setShowArchived((prev) => !prev)}
        aria-pressed={showArchived}
        className={`ml-auto shrink-0 rounded-md px-2 py-0.5 text-xs font-medium transition-colors ${
          showArchived
            ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
            : 'text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'
        }`}
      >
        {showArchived ? 'Archived' : 'Active'}
      </button>
    </div>
  )
}

const IMPORTANCE_SECTIONS: { value: ImportanceSection; label: string; description: string }[] = [
  { value: null, label: 'All', description: 'All conversations' },
  { value: 'action', label: 'Action', description: 'Requires your action' },
  { value: 'important', label: 'Important', description: 'Important messages' },
  { value: 'other', label: 'Other', description: 'Low priority & noise' },
]

function ImportanceSectionTabs() {
  const [section, setSection] = useAtom(importanceSectionAtom)
  const conversations = useAtomValue(conversationsAtom)

  const counts = useMemo(() => {
    let action = 0
    let important = 0
    let other = 0
    for (const c of conversations) {
      const lvl = c.importance_level
      if (lvl === 'critical' || lvl === 'important') {
        important++
      } else if (lvl === 'low' || lvl === 'noise') {
        other++
      }
      // count action-requiring separately (can overlap)
      // we check unread + importance for action tab
    }
    // action count: critical importance or unread important
    action = conversations.filter(
      (c) => c.importance_level === 'critical' || (c.importance_level === 'important' && c.unread_count > 0)
    ).length
    return { action, important, other }
  }, [conversations])

  return (
    <div className="flex gap-0.5 border-b border-zinc-200 px-3 py-1.5 dark:border-zinc-800" role="tablist" aria-label="Importance filter">
      {IMPORTANCE_SECTIONS.map((s) => {
        const count = s.value === 'action' ? counts.action : s.value === 'important' ? counts.important : s.value === 'other' ? counts.other : 0
        return (
          <button
            key={s.value ?? 'all'}
            role="tab"
            aria-selected={section === s.value}
            aria-label={s.description}
            onClick={() => setSection(section === s.value ? null : s.value)}
            className={`shrink-0 rounded-md px-2 py-0.5 text-xs font-medium transition-colors ${
              section === s.value
                ? s.value === 'action'
                  ? 'bg-red-600 text-white dark:bg-red-500'
                  : 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
                : 'text-zinc-500 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800'
            }`}
          >
            {s.label}
            {s.value && count > 0 && (
              <span className="ml-1 opacity-70">{count}</span>
            )}
          </button>
        )
      })}
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
    <div className="flex select-none items-center gap-3 px-4 py-1.5">
      <div className="h-px flex-1 bg-zinc-200 dark:bg-zinc-700" />
      <span className="shrink-0 text-[11px] font-medium text-zinc-400">{label}</span>
      <div className="h-px flex-1 bg-zinc-200 dark:bg-zinc-700" />
    </div>
  )
}

function ConversationSkeleton() {
  return (
    <div className="animate-pulse">
      {Array.from({ length: 8 }).map((_, i) => (
        <div key={i} className="flex items-start gap-3 px-4 py-3">
          <div className="h-9 w-9 shrink-0 rounded-full bg-zinc-200 dark:bg-zinc-700" />
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex items-center justify-between">
              <div className="h-3.5 w-24 rounded bg-zinc-200 dark:bg-zinc-700" />
              <div className="h-3 w-10 rounded bg-zinc-200 dark:bg-zinc-700" />
            </div>
            <div className="h-3 w-40 rounded bg-zinc-200 dark:bg-zinc-700" />
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
    <div className="absolute bottom-0 left-0 right-0 z-40 border-t border-zinc-200 bg-white/95 px-3 py-2 backdrop-blur dark:border-zinc-700 dark:bg-zinc-900/95">
      <div className="flex items-center gap-2">
        <span className="shrink-0 text-xs font-medium text-zinc-600 dark:text-zinc-400">
          {selectedCount} selected
        </span>
        <div className="flex flex-1 items-center gap-1.5 overflow-x-auto">
          <button
            onClick={() => onAction('read')}
            disabled={loading}
            className="shrink-0 rounded px-2.5 py-1 text-xs font-medium text-zinc-700 transition-colors hover:bg-zinc-100 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:text-zinc-300 dark:hover:bg-zinc-800"
          >
            Mark read
          </button>
          <button
            onClick={() => onAction('unread')}
            disabled={loading}
            className="shrink-0 rounded px-2.5 py-1 text-xs font-medium text-zinc-700 transition-colors hover:bg-zinc-100 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:text-zinc-300 dark:hover:bg-zinc-800"
          >
            Mark unread
          </button>
          <button
            onClick={() => onAction('star')}
            disabled={loading}
            className="shrink-0 rounded px-2.5 py-1 text-xs font-medium text-zinc-700 transition-colors hover:bg-zinc-100 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:text-zinc-300 dark:hover:bg-zinc-800"
          >
            Star
          </button>
          <button
            onClick={() => onAction('archive')}
            disabled={loading}
            className="shrink-0 rounded px-2.5 py-1 text-xs font-medium text-zinc-700 transition-colors hover:bg-zinc-100 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:text-zinc-300 dark:hover:bg-zinc-800"
          >
            Archive
          </button>
          <button
            onClick={() => onAction('delete')}
            disabled={loading}
            className="shrink-0 rounded px-2.5 py-1 text-xs font-medium text-red-600 transition-colors hover:bg-red-50 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:text-red-400 dark:hover:bg-red-900/20"
          >
            Delete
          </button>
        </div>
        <button
          onClick={onCancel}
          disabled={loading}
          className="shrink-0 rounded px-2.5 py-1 text-xs font-medium text-zinc-500 transition-colors hover:bg-zinc-100 disabled:opacity-50 dark:text-zinc-400 dark:hover:bg-zinc-800"
        >
          Cancel
        </button>
        {loading && (
          <div className="h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-zinc-300 border-t-zinc-600 dark:border-zinc-600 dark:border-t-zinc-300" />
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
      if (action === 'pin' || action === 'unpin' || action === 'archive' || action === 'unarchive') {
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
      <div className="flex items-center gap-2 border-b border-zinc-200 p-3 dark:border-zinc-800">
        <div role="search" className="relative flex-1">
          <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-400" aria-hidden="true" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            aria-label="Search conversations"
            className="w-full rounded-md border border-zinc-200 bg-zinc-50 py-1.5 pl-9 pr-3 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:border-zinc-500"
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
            className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors ${
              batchMode
                ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400'
                : 'text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800'
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
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-zinc-500 transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800"
          title="New conversation"
        >
          <SquarePen className="h-5 w-5" aria-hidden="true" />
        </button>
      </div>

      <QuickFilterBar />
      <DomainSelector />
      <ImportanceSectionTabs />
      <CategoryChips />
      <SortAndArchivedRow />

      <div
        ref={scrollContainerRef}
        role="list"
        aria-label="Conversations"
        className={`flex-1 overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
      >
        {initialLoading && conversations.length === 0 ? (
          <ConversationSkeleton />
        ) : conversations.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-8 text-center text-zinc-400">
            <Mail className="mb-3 h-10 w-10 text-zinc-300 dark:text-zinc-600" strokeWidth={1} aria-hidden="true" />
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
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-zinc-300 border-t-zinc-600 dark:border-zinc-600 dark:border-t-zinc-300" />
            )}
          </div>
        )}

        {!hasMore && conversations.length > 0 && (
          <div className="py-3 text-center text-xs text-zinc-400">
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
