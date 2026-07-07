import type { ContextMenuItem } from '@/components/context-menu'
import type { ConversationSummary } from '@/lib/types'

import { toast } from '@goliapkg/gds'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { Check, CheckCircle, Mail, MailCheck, Pin, Search, SquarePen, Star, X } from 'lucide-react'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { CategoryBadge, ImportanceBadge } from '@/components/category-badge'
import { ActionSheet, ContextMenu, useContextMenu } from '@/components/context-menu'
import { BatchActionBar } from '@/components/conversation-list-batch-action-bar'
import { FilterBar } from '@/components/conversation-list-filter-bar'
import { SenderAvatar } from '@/components/sender-avatar'
import { SwipeableRow } from '@/components/swipeable-row'
import { useCurrentMailFilters } from '@/hooks/use-current-mail-filters'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import {
  useArchiveMutation,
  useDeleteMutation,
  useMarkReadMutation,
  useMarkUnreadMutation,
  usePinMutation,
  useSnoozeMutation,
  useStarMutation,
  useUnarchiveMutation,
  useUnpinMutation,
  useUnstarMutation,
} from '@/hooks/use-mail-mutations'
import { postJson } from '@/lib/api'
import { extractEmail, extractName } from '@/lib/avatar'
import { dateGroupLabel, formatDate, formatFullDate } from '@/lib/format'
import { authAtom } from '@/store/auth'
import {
  batchModeAtom,
  composeReplySourceAtom,
  composingNewAtom,
  conversationsAtom,
  folderAtom,
  importanceSectionAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedThreadIdAtom,
  selectedThreadIdsAtom,
  showArchivedAtom,
  sortOrderAtom,
  stickyUnreadIdsAtom,
  visibleConversationIdsAtom,
} from '@/store/chat'

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

  // Stable references for the memo'd item — without useMemo these rebuild
  // every render, defeating React.memo and forcing the entire list of 50+
  // rows to re-render on every parent update (WebSocket tick, hover on
  // another row when group-hover used to be useState, etc).
  const contextItems = useMemo<ContextMenuItem[]>(
    () => [
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
    ],
    [convo.thread_id, hasUnread, isFlagged, isPinned, isArchived, onContextAction]
  )

  const handleClick = () => {
    if (batchMode) {
      onToggleCheck(convo.thread_id)
    } else {
      onSelect(convo.thread_id)
    }
  }

  return (
    <div
      className="group"
      onTouchEnd={ctx.onTouchEnd}
      onTouchMove={ctx.onTouchMove}
      onTouchStart={ctx.onTouchStart}
      role="listitem"
    >
      <button
        aria-label={`${name}: ${convo.subject || '(no subject)'}${hasUnread ? `, ${convo.unread_count} unread` : ''}${isPinned ? ', pinned' : ''}`}
        aria-selected={selected && !batchMode}
        // h-24 (96px) — HARD-FIXED row height. The previous design let
        // the row collapse when convo.snippet was empty, which mixed
        // two row-heights into the same list and broke the virtualizer's
        // dynamic-size measureElement path (measureElement race +
        // selected-state re-measure + absolute-positioned siblings ⇒
        // intermittent row overlap, see classic-errors.md). With a
        // fixed height the virtualizer never has to re-measure anything,
        // so the overlap bug class is eliminated by construction —
        // no patch, no hack.
        className={getRowClass({ batchMode, checked, hasUnread, selected })}
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
            <span className={getSenderClass({ hasUnread, isOwn })}>
              {name}
              {convo.participants.length > 1 && (
                <span className="text-fg-muted"> +{convo.participants.length - 1}</span>
              )}
            </span>
            <div className="flex shrink-0 items-center gap-1.5">
              {convo.message_count > 1 && (
                <span
                  className="bg-bg-secondary text-fg-muted rounded px-1 py-px text-xs tabular-nums md:text-[10px]"
                  title={`${convo.received_count} received · ${convo.sent_count} sent`}
                >
                  {convo.sent_count > 0 && convo.received_count > 0 ? (
                    <>
                      {convo.received_count}↓ {convo.sent_count}↑
                    </>
                  ) : (
                    convo.message_count
                  )}
                </span>
              )}
              {isPinned && <Pin className="text-accent h-3 w-3" />}
              {/* mobile: always show action buttons; desktop: show on hover
                  (group-hover, no useState — keeps the row out of the
                  re-render path for hover changes). */}
              {!batchMode && (
                <span className="flex items-center gap-0.5 md:hidden">
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
                    className={getStarClass({ density: 'mobile', isFlagged })}
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
              {/* desktop: hover actions via group-hover */}
              {!batchMode ? (
                <span className="hidden items-center gap-0.5 md:group-hover:flex">
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
                    className={getStarClass({ density: 'desktop', isFlagged })}
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
// Spam = AI-derived category filter (categoryFilter='spam', see classify.rs).
// Junk = physical Junk mailbox (mb.name='Junk'), populated by sieve / "mark spam" action.

const dateLabel = dateGroupLabel

type VirtualListItem =
  | { convo: ConversationSummary; type: 'conversation' }
  | { label: string; type: 'divider' }
  | { type: 'end' }
  | { type: 'sentinel' }

// module-level: survives component unmount/remount on mobile view switching.
// also persisted to sessionStorage so it survives a full page refresh.
const SCROLL_STORAGE_KEY = 'chat:list-scroll'
let savedScrollTop = (() => {
  try {
    const raw = sessionStorage.getItem(SCROLL_STORAGE_KEY)
    return raw ? Number(raw) || 0 : 0
  } catch {
    return 0
  }
})()
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
  // v2.1 phase-5b: reader migrated off `conversationsAtom` — the
  // component now reads the same conversations directly from the
  // `conversationKeys.infinite(...)` cache line the mail-list query
  // owns. `setConversations` still comes from the atom (Phase 5c
  // will redirect the writes to RQ), and the sync effect in
  // `chat.tsx` keeps atom and RQ aligned so unread badges from BOTH
  // stores agree during the transition.
  const filters = useCurrentMailFilters()
  const { conversations, hasMore, initialLoading, loadingMore } = useFlatConversations(filters)
  const setConversations = useSetAtom(conversationsAtom)
  const [selectedId, setSelectedId] = useAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
  const [searchQuery, setSearchQuery] = useAtom(searchQueryAtom)

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

  // keep savedScrollTop / sessionStorage in sync with actual scroll position
  // so a page refresh can put us back where we were.
  useEffect(() => {
    const el = scrollContainerRef.current
    if (!el) return
    let rAFid: null | number = null
    const onScroll = () => {
      if (rAFid != null) return
      rAFid = requestAnimationFrame(() => {
        rAFid = null
        persistScroll(el.scrollTop)
      })
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      el.removeEventListener('scroll', onScroll)
      if (rAFid != null) cancelAnimationFrame(rAFid)
    }
  }, [])

  // scroll restore: wait until conversations actually populate before
  // applying the saved scrollTop — otherwise the scroll container has
  // no content height yet and the assignment clamps to 0.
  const scrollRestoredRef = useRef(false)
  useEffect(() => {
    if (scrollRestoredRef.current) return
    if (conversations.length === 0) return
    const el = scrollContainerRef.current
    if (!el) return
    if (savedScrollTop <= 0) {
      scrollRestoredRef.current = true
      return
    }
    // give the virtualizer a frame to compute its total height
    const target = savedScrollTop
    requestAnimationFrame(() => {
      const node = scrollContainerRef.current
      if (node) node.scrollTop = target
      scrollRestoredRef.current = true
    })
  }, [conversations.length])

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

  // single-thread context menu action — each individual mutation runs
  // its own optimistic-update + rollback cycle inside react-query, so this
  // dispatcher only routes by action name. Toast messages remain here so
  // the visual feedback matches the human-facing language.
  const markReadMutation = useMarkReadMutation()
  const markUnreadMutation = useMarkUnreadMutation()
  const starMutation = useStarMutation()
  const unstarMutation = useUnstarMutation()
  const pinMutation = usePinMutation()
  const unpinMutation = useUnpinMutation()
  const archiveMutation = useArchiveMutation()
  const unarchiveMutation = useUnarchiveMutation()
  const snoozeMutation = useSnoozeMutation()
  const deleteMutation = useDeleteMutation()
  const handleContextAction = useCallback(
    async (threadId: string, action: SingleAction) => {
      const onError = (err: unknown) => {
        toast.error(err instanceof Error ? err.message : 'Action failed')
      }
      switch (action) {
        case 'archive':
          archiveMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Archived') }
          )
          break
        case 'delete':
          deleteMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Deleted') }
          )
          break
        case 'pin':
          pinMutation.mutate({ threadId }, { onError, onSuccess: () => toast.success('Pinned') })
          break
        case 'read':
          markReadMutation.mutate({ threadId }, { onError })
          break
        case 'snooze': {
          const tomorrow = new Date()
          tomorrow.setDate(tomorrow.getDate() + 1)
          tomorrow.setHours(9, 0, 0, 0)
          snoozeMutation.mutate(
            { threadId, until: tomorrow.toISOString() },
            { onError, onSuccess: () => toast.success('Snoozed until tomorrow 9:00') }
          )
          break
        }
        case 'star':
          starMutation.mutate({ threadId }, { onError })
          break
        case 'unarchive':
          unarchiveMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Unarchived') }
          )
          break
        case 'unpin':
          unpinMutation.mutate(
            { threadId },
            { onError, onSuccess: () => toast.success('Unpinned') }
          )
          break
        case 'unread':
          markUnreadMutation.mutate({ threadId }, { onError })
          break
        case 'unstar':
          unstarMutation.mutate({ threadId }, { onError })
          break
      }
    },
    [
      archiveMutation,
      deleteMutation,
      markReadMutation,
      markUnreadMutation,
      pinMutation,
      snoozeMutation,
      starMutation,
      unarchiveMutation,
      unpinMutation,
      unstarMutation,
    ]
  )

  const sortOrder = useAtomValue(sortOrderAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const quickFilter = useAtomValue(quickFilterAtom)
  const folder = useAtomValue(folderAtom)
  const [stickyUnread, setStickyUnread] = useAtom(stickyUnreadIdsAtom)

  // Reset the "keep visible until next visit" set whenever the user
  // navigates AWAY from the unread filter — the set was scoped to the
  // current unread session. We also clear it on unmount via the cleanup
  // returned below so leaving /mail entirely starts a fresh session.
  useEffect(() => {
    if (quickFilter !== 'unread' && stickyUnread.size > 0) {
      setStickyUnread(new Set())
    }
  }, [quickFilter, stickyUnread, setStickyUnread])
  useEffect(
    () => () => {
      setStickyUnread(new Set())
    },
    [setStickyUnread]
  )

  // apply client-side filtering + sort
  const sortedConversations = useMemo(() => {
    let visible = showArchived ? conversations : conversations.filter((c) => !c.archived)

    // "hide my own latest sends from All" is enforced by the server in
    // list_conversations when folder != Sent; no client filter needed

    // quick filter
    if (quickFilter === 'unread') {
      // Gmail-style: a thread marked-read while the user is sitting on this
      // filter stays visible until they leave the filter (the row should
      // never just vanish under the cursor). stickyUnread is cleared by the
      // useEffect above when quickFilter flips off 'unread'.
      visible = visible.filter((c) => c.unread_count > 0 || stickyUnread.has(c.thread_id))
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
  }, [conversations, sortOrder, showArchived, importanceSection, quickFilter, stickyUnread])

  // sync visible conversation ids to store for keyboard nav. Compare order
  // before writing to avoid replacing the atom (and re-rendering every
  // subscriber, e.g. ThreadView) when the list shape is unchanged but the
  // array reference flipped from a WebSocket-driven refetch.
  const setVisibleIds = useSetAtom(visibleConversationIdsAtom)
  useEffect(() => {
    setVisibleIds((prev) => {
      const next = sortedConversations.map((c) => c.thread_id)
      if (prev.length === next.length && prev.every((v, i) => v === next[i])) return prev
      return next
    })
  }, [sortedConversations, setVisibleIds])

  // stable callbacks that accept threadId to avoid inline closures in the map
  const handleSelect = useCallback(
    (threadId: string) => {
      // save scroll position before navigating to thread (also persists to
      // sessionStorage so a refresh from the thread view restores list scroll)
      if (scrollContainerRef.current) {
        persistScroll(scrollContainerRef.current.scrollTop)
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
              try {
                const resp = await postJson<{ flipped: number; success: boolean }>(
                  '/conversations/mark-all-read',
                  {}
                )
                setConversations((prev) => prev.map((c) => ({ ...c, unread_count: 0 })))
                toast.success(`Marked ${resp.flipped ?? 0} as read`)
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
            setComposeReplySource(null)
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

function DateDivider({ label }: { label: string }) {
  return (
    <div className="sticky top-0 z-10 flex justify-center py-1.5 select-none">
      <span className="bg-bg-secondary text-fg-muted rounded-full px-2.5 py-0.5 text-xs font-medium md:text-[10px]">
        {label}
      </span>
    </div>
  )
}

function persistScroll(value: number) {
  savedScrollTop = value
  try {
    if (value > 0) sessionStorage.setItem(SCROLL_STORAGE_KEY, String(value))
    else sessionStorage.removeItem(SCROLL_STORAGE_KEY)
  } catch {
    // ignore quota / privacy mode
  }
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
    // Fixed per-type heights. Matches the CSS h-24 / h-8 / h-12 on
    // ConversationItem / DateDivider / sentinel + end markers.
    //
    // NOTE: this used to be `estimateSize` paired with
    // `ref={virtualizer.measureElement}` for dynamic-size mode. That
    // path turns out to be fundamentally racy when combined with
    // absolute-positioned children — selected-state re-renders fire
    // measureElement again, the virtualizer cache updates, but
    // already-rendered siblings keep their stale `translateY`. Visible
    // result: row overlap (classic-errors.md "react-virtual on a
    // WebSocket-fed list MUST pass getItemKey" entry was a partial
    // fix; the real fix is below — kill the dynamic-size path
    // entirely so there is nothing to race against).
    estimateSize: (index) => {
      const item = items[index]
      if (item.type === 'divider') return 32
      if (item.type === 'sentinel' || item.type === 'end') return 48
      return 96 // matches `h-24` on the row button
    },
    getScrollElement: () => parentRef.current,
    // Stable per-logical-item key so the virtualizer's internal cache
    // moves with the data when items are inserted / sorted by a WS
    // push. Still needed: even fixed-size virtualizers use this for
    // identity tracking of the scroll position. Keep in sync with the
    // React key applied a few lines below.
    getItemKey: (index) => {
      const item = items[index]
      if (item.type === 'conversation') return `c:${item.convo.thread_id}`
      if (item.type === 'divider') return `d:${item.label}`
      return item.type
    },
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
        aria-busy="true"
        className={`flex flex-1 items-center justify-center overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
        ref={scrollContainerRef}
        role="list"
      >
        <div
          aria-label="Loading conversations"
          className="border-border border-t-accent h-8 w-8 animate-spin rounded-full border-2"
        />
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
                    : folder === 'Junk'
                      ? 'No junk mail'
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
          // Stable key per logical item, not per virtual slot index. When
          // a new conversation arrives at the top (or sort changes), the
          // index-based key reused the same React tree for a different
          // conversation, blowing away ConversationItem's internal
          // useContextMenu state and forcing a full row remount even
          // though the same DOM could have moved.
          const itemKey =
            item.type === 'conversation'
              ? `c:${item.convo.thread_id}`
              : item.type === 'divider'
                ? `d:${item.label}`
                : item.type
          return (
            <div
              // No `ref={virtualizer.measureElement}` — fixed-size mode
              // (see useVirtualizer config above). The estimateSize
              // value IS the row height; nothing to measure, nothing
              // to race against.
              className="absolute top-0 left-0 w-full"
              data-index={virtualItem.index}
              key={itemKey}
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

// row outer class — pulled out so the JSX above stops being a 9-line
// ternary salad; the four input bools map to the same 5-token output every
// render, so a pure function is the clean place for it.
const ROW_BASE =
  'focus-visible:ring-accent/50 relative flex h-24 w-full items-start gap-3 overflow-hidden border-l-[3px] px-4 py-2.5 text-left transition-all duration-150 focus-visible:ring-2 focus-visible:outline-none'

function getRowClass({
  batchMode,
  checked,
  hasUnread,
  selected,
}: {
  batchMode: boolean
  checked: boolean
  hasUnread: boolean
  selected: boolean
}): string {
  const isSelected = selected && !batchMode
  const border = isSelected ? 'border-l-accent' : 'border-l-transparent'
  const bg = isSelected || checked ? 'bg-accent/10' : 'hover:bg-bg-secondary'
  const dim = !hasUnread && !selected && !checked ? 'opacity-70 hover:opacity-100' : ''
  return `${ROW_BASE} ${border} ${bg} ${dim}`
}

function getSenderClass({ hasUnread, isOwn }: { hasUnread: boolean; isOwn: boolean }): string {
  // hasUnread wins over isOwn — same effective cascade as the previous
  // double-ternary (`text-accent text-fg ...` resolves to the last token).
  const color = hasUnread ? 'text-fg font-semibold' : isOwn ? 'text-accent' : 'text-fg-secondary'
  return `truncate text-sm ${color}`
}

function getStarClass({
  density,
  isFlagged,
}: {
  density: 'desktop' | 'mobile'
  isFlagged: boolean
}): string {
  const base =
    density === 'mobile' ? 'touch-target rounded p-1' : 'hover:bg-bg-secondary rounded p-0.5'
  const color = isFlagged ? 'text-warning' : 'text-fg-muted hover:text-fg-secondary'
  return `${base} ${color}`
}
