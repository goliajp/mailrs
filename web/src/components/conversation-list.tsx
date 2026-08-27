export { DateDivider } from '@/components/conversation-list-virtual'

import type { BatchAction } from '@/components/conversation-actions'

import { toast } from '@goliapkg/gds'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { CheckCircle, MailCheck, SquarePen } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { BatchActionBar } from '@/components/conversation-list-batch-action-bar'
import { FilterBar } from '@/components/conversation-list-filter-bar'
import { VirtualConversationList } from '@/components/conversation-list-virtual'
import { DeleteThreadConfirm } from '@/components/delete-thread-confirm'
import { ListSearchInput } from '@/components/list-search-input'
import { useConversationActions } from '@/hooks/use-conversation-actions'
import { useConversationRows, useCurrentSelection } from '@/hooks/use-current-list'
import { useCurrentMailFilters } from '@/hooks/use-current-mail-filters'
import { dateGroupLabel } from '@/lib/format'
import { listIdentity } from '@/lib/list-identity'
import { persistScroll, readSavedScroll } from '@/lib/list-scroll'
import { threadAxesOf } from '@/lib/mail-lists'
import { queryClient } from '@/lib/query-client'
import { authAtom } from '@/store/auth'
import { conversationKeys } from '@/store/query-keys-v21'
import { activeListAtom } from '@/store/ui'
import {
  batchModeAtom,
  composeReplySourceAtom,
  composingNewAtom,
  folderAtom,
  pickInListAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedThreadIdsAtom,
  sortOrderAtom,
  stickyUnreadIdsAtom,
} from '@/store/ui'
import { wireBatchMutation, wireMarkAllRead } from '@/wire/endpoints/mutations'

export function ConversationList({ onSelectConversation }: { onSelectConversation?: () => void }) {
  const auth = useAtomValue(authAtom)
  const myEmail = auth?.address ?? ''
  // The rows and the current one both come from `use-current-list`, so
  // the reading pane beside this list is looking at the same list. The
  // narrowing that used to be a `useMemo` here is `narrowConversations`,
  // out where both can call it.
  const filters = useCurrentMailFilters()
  const {
    error,
    hasMore,
    initialLoading,
    isError,
    loadingMore,
    loadMore,
    refresh,
    rows: sortedConversations,
  } = useConversationRows()
  const selectedId = useCurrentSelection()?.threadId ?? null
  const pickRow = useSetAtom(pickInListAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
  const searchQuery = useAtomValue(searchQueryAtom)

  // batch mode state
  const [batchMode, setBatchMode] = useAtom(batchModeAtom)
  const [selectedThreadIds, setSelectedThreadIds] = useAtom(selectedThreadIdsAtom)
  const [batchLoading, setBatchLoading] = useState(false)
  const [pendingBatchDelete, setPendingBatchDelete] = useState<null | string[]>(null)

  // refs to avoid stale closures in observer callback
  const onLoadMoreRef = useRef(loadMore)
  onLoadMoreRef.current = loadMore
  const loadingRef = useRef(loadingMore)
  loadingRef.current = loadingMore

  // observer ref to clean up when sentinel unmounts
  const observerRef = useRef<IntersectionObserver | null>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  // Which list is showing. The saved scroll position is per-list.
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

  // Switching lists starts at the top.
  //
  // The component does not remount when the list changes, so without this
  // the container keeps the offset it had: scrolling the Inbox and then
  // opening Sent showed the middle of Sent. Scrolling is an action, which
  // is why it is still an effect — the selection is not, and the two used
  // to be done together here.
  const scrollRestoredRef = useRef(false)
  useEffect(() => {
    if (identityRef.current === identity) return
    identityRef.current = identity
    scrollRestoredRef.current = true
    const el = scrollContainerRef.current
    if (el) el.scrollTop = 0
    persistScroll(identity, 0)
  }, [identity])

  // scroll restore: wait until conversations actually populate before
  // applying the saved scrollTop — otherwise the scroll container has
  // no content height yet and the assignment clamps to 0.
  useEffect(() => {
    if (scrollRestoredRef.current) return
    if (sortedConversations.length === 0) return
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
  }, [sortedConversations.length, identity])

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
  // The wire call, separated from the decision to make it: the delete
  // path has to run this *after* an answer, and it was one closure.
  const runBatch = useCallback(
    async (action: BatchAction, ids: string[]) => {
      setBatchLoading(true)
      try {
        const result = await wireBatchMutation(action, ids)
        // How many, not "some": the route reports which ids it could
        // not do, and a message that cannot count is one the reader
        // has to go and check for themselves.
        const refused = result.failed_thread_ids.length || result.failed
        const msg =
          result.message ?? (result.success ? 'Done' : `${refused} of ${ids.length} failed`)
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
    [exitBatchMode]
  )

  const handleBatchAction = useCallback(
    async (action: BatchAction) => {
      const ids = Array.from(selectedThreadIds)
      if (ids.length === 0) return
      // Held for an answer, like the single-thread verb beside it. It
      // went straight to the wire, so forty threads were unlinked from
      // disk on one click while deleting one of them from its own row
      // asked first.
      if (action === 'delete') {
        setPendingBatchDelete(ids)
        return
      }
      await runBatch(action, ids)
    },
    [selectedThreadIds, runBatch]
  )

  const actions = useConversationActions()

  const setSortOrder = useSetAtom(sortOrderAtom)
  const quickFilter = useAtomValue(quickFilterAtom)
  const activeList = useAtomValue(activeListAtom)
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

  // stable callbacks that accept threadId to avoid inline closures in the map
  const handleSelect = useCallback(
    (threadId: string) => {
      // save scroll position before navigating to thread (also persists to
      // sessionStorage so a refresh from the thread view restores list scroll)
      if (scrollContainerRef.current) {
        persistScroll(identityRef.current, scrollContainerRef.current.scrollTop)
      }
      pickRow({ threadId, uid: null })
      setComposingNew(false)
      onSelectConversation?.()
    },
    [pickRow, setComposingNew, onSelectConversation]
  )

  const isSearching = searchQuery.trim().length > 0
  const hasBatchBar = batchMode && selectedThreadIds.size > 0

  return (
    <div className="relative flex h-full flex-col select-none">
      <ListSearchInput label="Search conversations">
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
            <CheckCircle aria-hidden="true" className="h-4 w-4" />
          </button>
        )}

        {sortedConversations.some((c) => c.unread_count > 0) && (
          <button
            aria-label="Mark all as read"
            className="text-fg-muted hover:bg-bg-secondary flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150"
            onClick={async () => {
              try {
                // Scoped to the list on screen, with the same axes the
                // list read sends — "mark all as read" pressed inside
                // Notifications must not silence the inbox.
                const resp = await wireMarkAllRead(threadAxesOf(activeList) ?? undefined)
                // Refetched, not patched. The old code zeroed every
                // cached list, which was right when this marked the
                // whole mailbox and is a lie now: the server touched
                // one list, and rows this client has never loaded, so
                // the only honest local state is the one it asks for
                // again.
                await queryClient.invalidateQueries({ queryKey: conversationKeys.infinites() })
                toast.success(`Marked ${resp.flipped ?? 0} as read`)
              } catch {
                toast.error('Failed')
              }
            }}
            title="Mark all as read"
          >
            <MailCheck aria-hidden="true" className="h-4 w-4" />
          </button>
        )}

        <button
          aria-label="New conversation"
          className="text-fg-muted hover:bg-bg-secondary flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150"
          onClick={() => {
            setComposeReplySource(null)
            setComposingNew(true)
          }}
          title="New conversation"
        >
          <SquarePen aria-hidden="true" className="h-4 w-4" />
        </button>
      </ListSearchInput>

      <FilterBar />

      <DeleteThreadConfirm
        onCancel={actions.cancelDelete}
        onConfirm={actions.confirmDelete}
        open={actions.pendingDelete !== null}
      />

      <DeleteThreadConfirm
        count={pendingBatchDelete?.length ?? 0}
        onCancel={() => setPendingBatchDelete(null)}
        onConfirm={() => {
          const ids = pendingBatchDelete
          setPendingBatchDelete(null)
          if (ids) void runBatch('delete', ids)
        }}
        open={pendingBatchDelete !== null}
      />

      <VirtualConversationList
        batchMode={batchMode}
        conversations={sortedConversations}
        dateLabel={dateGroupLabel}
        error={error}
        folder={folder}
        hasBatchBar={hasBatchBar}
        hasMore={hasMore}
        initialLoading={initialLoading}
        isError={isError}
        isSearching={isSearching}
        loadingMore={loadingMore}
        myEmail={myEmail}
        onContextAction={actions.act}
        onLoadMore={sentinelCallback}
        onRefresh={refresh}
        onSelect={handleSelect}
        onToggleCheck={toggleThreadCheck}
        scrollContainerRef={scrollContainerRef}
        selectedId={selectedId}
        selectedThreadIds={selectedThreadIds}
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
