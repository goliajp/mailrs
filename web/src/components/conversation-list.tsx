export { DateDivider } from '@/components/conversation-list-virtual'

import type { BatchAction } from '@/components/conversation-actions'

import { toast } from '@goliapkg/gds'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { CheckCircle, MailCheck, Search, SquarePen, X } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { BatchActionBar } from '@/components/conversation-list-batch-action-bar'
import { FilterBar } from '@/components/conversation-list-filter-bar'
import { VirtualConversationList } from '@/components/conversation-list-virtual'
import { useConversationActions } from '@/hooks/use-conversation-actions'
import { useCurrentMailFilters } from '@/hooks/use-current-mail-filters'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import { dateGroupLabel } from '@/lib/format'
import { listIdentity } from '@/lib/list-identity'
import { persistScroll, readSavedScroll } from '@/lib/list-scroll'
import { queryClient } from '@/lib/query-client'
import { patchAllInfiniteLists } from '@/reducers/snapshot'
import { authAtom } from '@/store/auth'
import {
  batchModeAtom,
  composeReplySourceAtom,
  composingNewAtom,
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
} from '@/store/ui'
import { wireBatchMutation, wireMarkAllRead } from '@/wire/endpoints/mutations'

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
  // v2.1 phase-5b/c: reader migrated off `conversationsAtom` — the
  // component reads the same conversations directly from the
  // `conversationKeys.infinite(...)` cache line the mail-list query
  // owns. The mark-all-read writer (see line ~720) patches the RQ
  // cache via `patchAllInfiniteLists`; every screen subscribing to
  // that cache re-renders together on the next paint.
  const filters = useCurrentMailFilters()
  const { conversations, hasMore, initialLoading, loadingMore } = useFlatConversations(filters)
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

  // Which list is showing. Scroll and selection both reset off this.
  const identity = useMemo(() => listIdentity(filters), [filters])
  const identityRef = useRef(identity)

  // keep the saved scroll in sync with the actual position so a page
  // refresh puts us back where we were — under this list's own key.
  useEffect(() => {
    const el = scrollContainerRef.current
    if (!el) return
    let rAFid: null | number = null
    const onScroll = () => {
      if (rAFid != null) return
      rAFid = requestAnimationFrame(() => {
        rAFid = null
        persistScroll(identityRef.current, el.scrollTop)
      })
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      el.removeEventListener('scroll', onScroll)
      if (rAFid != null) cancelAnimationFrame(rAFid)
    }
  }, [])

  // Switching lists starts at the top with the first message selected.
  //
  // The component does not remount when the folder changes, so without this
  // the container keeps the offset it had: scrolling the Inbox and then
  // opening Sent showed the middle of Sent. The selection carried over the
  // same way — a thread from the Inbox stayed open above the Sent list.
  //
  // `setSelectedId` and not the click handler: the handler also switches the
  // mobile view to the thread, which would drag a phone user into a message
  // they did not open.
  const scrollRestoredRef = useRef(false)
  useEffect(() => {
    if (identityRef.current === identity) return
    identityRef.current = identity
    scrollRestoredRef.current = true
    const el = scrollContainerRef.current
    if (el) el.scrollTop = 0
    persistScroll(identity, 0)
    setSelectedId(null)
  }, [identity, setSelectedId])

  // With the list changed and the selection cleared, take the first row of
  // whatever arrived. Separate from the reset above because the rows are
  // not there yet when the identity changes.
  useEffect(() => {
    if (selectedId !== null) return
    const first = conversations[0]
    if (first) setSelectedId(first.thread_id)
  }, [conversations, selectedId, setSelectedId])

  // scroll restore: wait until conversations actually populate before
  // applying the saved scrollTop — otherwise the scroll container has
  // no content height yet and the assignment clamps to 0.
  useEffect(() => {
    if (scrollRestoredRef.current) return
    if (conversations.length === 0) return
    const el = scrollContainerRef.current
    if (!el) return
    const saved = readSavedScroll(identity)
    if (saved <= 0) {
      scrollRestoredRef.current = true
      return
    }
    // give the virtualizer a frame to compute its total height
    requestAnimationFrame(() => {
      const node = scrollContainerRef.current
      if (node) node.scrollTop = saved
      scrollRestoredRef.current = true
    })
  }, [conversations.length, identity])

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
        const result = await wireBatchMutation(action, ids)
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

  const handleContextAction = useConversationActions()

  const [sortOrder, setSortOrder] = useAtom(sortOrderAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const quickFilter = useAtomValue(quickFilterAtom)
  const folder = useAtomValue(folderAtom)
  const [stickyUnread, setStickyUnread] = useAtom(stickyUnreadIdsAtom)

  // Starting a search switches the order to relevance, and clearing it
  // switches back. Ranking a search by date discards the ranking, and
  // leaving the default at `newest` while the server ranked by score
  // was the state that made search order look arbitrary. Keyed on the
  // *transition* so an explicit choice during a search still sticks.
  const searching = searchQuery.trim().length > 0
  const wasSearching = useRef(searching)
  useEffect(() => {
    if (searching === wasSearching.current) return
    wasSearching.current = searching
    setSortOrder(searching ? 'relevance' : 'newest')
  }, [searching, setSortOrder])

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
    if (importanceSection === 'important') {
      visible = visible.filter(
        (c) => c.importance_level === 'critical' || c.importance_level === 'important'
      )
    } else if (importanceSection === 'other') {
      visible = visible.filter(
        (c) => c.importance_level === 'low' || c.importance_level === 'noise'
      )
    }

    // `relevance` means "leave the server's order alone". For a plain
    // list that order is already newest-first, so the two agree; for a
    // search it is the ranking, and sorting it by date would discard
    // the ranking. Everything else genuinely sorts — `newest` used to
    // return early here on the assumption the server had already done
    // it, which was true of the list and false of search, so the one
    // option named after a date was the only one that never applied one.
    if (sortOrder === 'relevance') return visible
    const pinned = visible.filter((c) => c.pinned)
    const unpinned = visible.filter((c) => !c.pinned)
    if (sortOrder === 'newest') {
      unpinned.sort((a, b) => b.last_date - a.last_date)
    } else if (sortOrder === 'oldest') {
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
        persistScroll(identityRef.current, scrollContainerRef.current.scrollTop)
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
                const resp = await wireMarkAllRead()
                // v2.1 phase-5c — patch the RQ cache directly. Every
                // reader subscribing to `conversationKeys.infinites()`
                // sees `unread_count = 0` on the next paint.
                patchAllInfiniteLists(queryClient, (c) => ({ ...c, unread_count: 0 }))
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
        dateLabel={dateGroupLabel}
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
// shared with the Sent / Drafts list views so every list groups rows
// under the same Today / Yesterday / weekday pills.
