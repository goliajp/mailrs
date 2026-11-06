import type { SingleAction } from '@/components/conversation-actions'
import type { SwipeSide } from '@/components/swipeable-row'
import type { SwipeChoice } from '@/lib/swipe'
import type { ConversationSummary } from '@/lib/types'

import { useVirtualizer } from '@tanstack/react-virtual'
import { useAtomValue } from 'jotai'
import { CloudOff, Mail } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { ConversationItem } from '@/components/conversation-item'
import { SwipeableRow } from '@/components/swipeable-row'
import { useHoverPrefetch } from '@/hooks/use-prefetch-threads'
import { useSectionedInbox } from '@/hooks/use-sectioned-inbox'
import { useSwipeSetting } from '@/hooks/use-swipe-setting'
import { type Key, useT } from '@/i18n'
import { nearEnd } from '@/lib/list-paging'
import { MAIL_LISTS, type MailListId } from '@/lib/mail-lists'
import { swipeAction } from '@/lib/swipe'
import { followupDaysAtom, type InboxSection, inboxSectionAtom } from '@/store/inbox-sections'
import { activeListAtom } from '@/store/ui'

export type VirtualListItem =
  | { anchor: string; label: string; type: 'divider' }
  // `anchor` = thread_id of the row right below the divider. Labels
  // repeat whenever the list isn't date-monotonic (pinned rows on top,
  // relevance-ordered search), and react-virtual keys MUST be unique —
  // duplicate `d:<label>` keys produced ghost dividers + blank gaps.
  | { convo: ConversationSummary; type: 'conversation' }
  | { type: 'end' }
  | { type: 'sentinel' }

export function DateDivider({ label }: { label: string }) {
  return (
    <div className="sticky top-0 z-10 flex justify-center py-1.5 select-none">
      <span className="bg-bg-secondary text-fg-muted md:text-tiny rounded-full px-2.5 py-0.5 text-xs font-medium">
        {label}
      </span>
    </div>
  )
}

// Scroll position, per list.
//
// Survives the component unmounting on a mobile view switch (module scope)
// and a full page refresh (sessionStorage). Keyed by which list is showing,
// because it used to be one variable and one fixed key for all of them:
// scrolling the Inbox and then opening Sent left Sent at the Inbox's offset,
// showing the middle of a list the user had never scrolled.

export function VirtualConversationList({
  batchMode,
  conversations,
  dateLabel,
  error,
  folder,
  hasBatchBar,
  hasMore,
  initialLoading,
  isError,
  isSearching,
  loadingMore,
  myEmail,
  onContextAction,
  onNearEnd,
  onRefresh,
  onSelect,
  onToggleCheck,
  scrollContainerRef,
  selectedId,
  selectedThreadIds,
}: {
  batchMode: boolean
  conversations: ConversationSummary[]
  dateLabel: (ts: number) => string
  error: unknown
  folder: null | string
  hasBatchBar: boolean
  hasMore: boolean
  initialLoading: boolean
  isError: boolean
  isSearching: boolean
  loadingMore: boolean
  myEmail: string
  onContextAction: (threadId: string, action: SingleAction) => void
  onNearEnd: () => void
  onRefresh?: () => Promise<void> | void
  onSelect: (threadId: string) => void
  onToggleCheck: (threadId: string) => void
  scrollContainerRef: React.RefObject<HTMLDivElement | null>
  selectedId: null | string
  selectedThreadIds: Set<string>
}) {
  const t = useT()
  const activeList = useAtomValue(activeListAtom)
  const sectioned = useSectionedInbox()
  const shownSection = useAtomValue(inboxSectionAtom)
  const followupDays = useAtomValue(followupDaysAtom)
  const inboxSection = sectionShown(sectioned, shownSection)
  // build flat list of items. Search results are relevance-ordered, not
  // date-monotonic — date group pills would repeat and read as noise, so
  // skip them entirely while searching.
  const items = useMemo<VirtualListItem[]>(() => {
    if (conversations.length === 0) return []
    const result: VirtualListItem[] = []
    let prevGroup = ''
    for (const c of conversations) {
      if (!isSearching) {
        const group = dateLabel(c.last_date)
        if (group !== prevGroup) {
          result.push({ anchor: c.thread_id, label: group, type: 'divider' })
          prevGroup = group
        }
      }
      result.push({ convo: c, type: 'conversation' })
    }
    if (hasMore) result.push({ type: 'sentinel' })
    else result.push({ type: 'end' })
    return result
  }, [conversations, dateLabel, hasMore, isSearching])

  const parentRef = useRef<HTMLDivElement>(null)
  const hover = useHoverPrefetch()
  const swipe = useSwipeSetting()

  const virtualizer = useVirtualizer({
    count: items.length,
    // Rows drawn beyond each edge of the screen. Every one of them is
    // mounted when a scroll jumps, and a wheel turn jumps a whole screen:
    // at ten, a jump mounted about thirty rows to show eleven. Rows are a
    // fixed height and drawn in the same frame as the scroll, so four is
    // enough to never show a blank one.
    overscan: 4,
    // Fixed per-type heights. Matches the CSS h-24 / h-8 / h-12 on
    // ConversationItem / DateDivider / sentinel + end markers.
    //
    // NOTE: this used to be `estimateSize` paired with
    // `ref={virtualizer.measureElement}` for dynamic-size mode. That
    // path turns out to be fundamentally racy when combined with
    // absolute-positioned children — selected-state re-renders fire
    // measureElement again, the virtualizer cache updates, but
    // already-rendered siblings keep their stale `translateY`. Visible
    // result: row overlap (passing getItemKey was a partial
    // fix; the real fix is below — kill the dynamic-size path
    // entirely so there is nothing to race against).
    estimateSize: (index) => {
      const item = items[index]
      if (item.type === 'divider') return 32
      if (item.type === 'sentinel' || item.type === 'end') return 48
      return 64 // matches `h-16` on the row button (compact two-line rows)
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
      if (item.type === 'divider') return `d:${item.anchor}`
      return item.type
    },
  })

  // the next page, asked for while there are still rows to scroll through
  const visibleEnd = virtualizer.range?.endIndex ?? -1
  useEffect(() => {
    if (!hasMore || loadingMore) return
    if (nearEnd(visibleEnd, items.length)) onNearEnd()
  }, [visibleEnd, items.length, hasMore, loadingMore, onNearEnd])

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
          aria-label={t('list.loading')}
          className="border-border border-t-accent h-8 w-8 animate-spin rounded-full border-2"
        />
      </div>
    )
  }

  // **Before the empty state, not after it.** A failed fetch used to
  // fall through to "All caught up!", so a server that was down told
  // the reader their mail was gone and gave them nothing to click.
  if (isError && conversations.length === 0) {
    return (
      <div
        className={`flex-1 overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
        ref={scrollContainerRef}
      >
        <div className="flex flex-col items-center justify-center p-8 text-center" role="alert">
          <CloudOff aria-hidden="true" className="text-danger mb-3 h-10 w-10" strokeWidth={1} />
          <p className="text-sm font-medium">{t('list.loadFailed')}</p>
          <p className="text-fg-muted mt-1 text-xs">
            {listErrorMessage(error, t('list.noAnswer'))}
          </p>
          <button
            className="border-border hover:bg-bg-secondary focus-visible:ring-accent/50 mt-3 rounded-md border px-3 py-1.5 text-xs transition-colors focus-visible:ring-2 focus-visible:outline-none"
            onClick={() => void onRefresh?.()}
            type="button"
          >
            {t('common.tryAgain')}
          </button>
        </div>
      </div>
    )
  }

  if (conversations.length === 0) {
    const hint = emptyHint(isSearching)
    return (
      <div
        className={`flex-1 overflow-y-auto ${hasBatchBar ? 'pb-14' : ''}`}
        ref={scrollContainerRef}
        role="list"
      >
        <div className="text-fg-muted flex flex-col items-center justify-center p-8 text-center">
          <Mail aria-hidden="true" className="text-fg-muted mb-3 h-10 w-10" strokeWidth={1} />
          <p className="text-sm font-medium">
            {t(emptyLabel(isSearching, activeList, inboxSection), { days: followupDays })}
          </p>
          {hint && <p className="mt-1 text-xs">{t(hint)}</p>}
        </div>
      </div>
    )
  }

  return (
    <div
      aria-label={t('list.conversations')}
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
                ? `d:${item.anchor}`
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
              onPointerEnter={() => {
                if (item.type === 'conversation') hover.enter(item.convo.thread_id)
              }}
              onPointerLeave={hover.leave}
              style={{ transform: `translateY(${virtualItem.start}px)` }}
            >
              {item.type === 'divider' && <DateDivider label={item.label} />}
              {item.type === 'conversation' && (
                <SwipeableRow
                  left={swipeSide(swipe.left, item.convo, onContextAction)}
                  right={swipeSide(swipe.right, item.convo, onContextAction)}
                >
                  <ConversationItem
                    batchMode={batchMode}
                    checked={selectedThreadIds.has(item.convo.thread_id)}
                    convo={item.convo}
                    isJunkView={folder === 'Junk'}
                    isNpView={folder === 'NP'}
                    myEmail={myEmail}
                    onContextAction={onContextAction}
                    onSelect={onSelect}
                    onToggleCheck={onToggleCheck}
                    selected={selectedId === item.convo.thread_id}
                  />
                </SwipeableRow>
              )}
              {item.type === 'sentinel' && (
                <div className="flex justify-center py-4">
                  {loadingMore && (
                    <div className="border-border border-t-fg-secondary h-5 w-5 animate-spin rounded-full border-2" />
                  )}
                </div>
              )}
              {item.type === 'end' && (
                <div className="text-fg-muted py-3 text-center text-xs">{t('list.end')}</div>
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
// compact two-line rows (2026-07-17): h-16, no snippet line, matching
// the Sent view's density. Height MUST stay in sync with the
// virtualizer's estimateSize (fixed-size mode — see the note there), which
// is why the base lives in `lib/list-row-class.ts` alongside the states —
// the Send view has the same row and had drifted from it.

/// The second line of the empty state, or nothing.
///
/// It used to be a ternary that rendered an empty string when not
/// searching — a `<p>` that was always mounted, always invisible, and
/// always contributing its top margin.
function emptyHint(isSearching: boolean): Key | null {
  if (isSearching) return 'list.search.hint'
  return null
}

/**
 * What an empty list says.
 *
 * One lookup on the list, not a chain of comparisons against the filters
 * it happens to imply — which is the same reverse-engineering the tab
 * resolver used to do, and it had already drifted: it answered "No sent
 * messages" for a folder the Send tab no longer sets, and had no arm at
 * all for Starred or N & P, so both said "All caught up!".
 */
function emptyLabel(isSearching: boolean, list: MailListId, section: InboxSection | null): Key {
  if (isSearching) return 'list.search.empty'
  if (list === 'inbox' && section) return SECTION_EMPTY[section]
  return MAIL_LISTS[list].emptyLabel
}

const SECTION_EMPTY: Record<InboxSection, Key> = {
  followup: 'sections.followup.empty',
  new: 'sections.new.empty',
  other: 'sections.other.empty',
  reply: 'sections.reply.empty',
}

/// What went wrong, in a sentence, without leaking an object.
function listErrorMessage(error: unknown, noAnswer: string): string {
  if (error instanceof Error && error.message.length > 0) return error.message
  return noAnswer
}

/** The Inbox section on screen, or none while the Inbox is shown whole. */
function sectionShown(sectioned: boolean, section: InboxSection): InboxSection | null {
  if (!sectioned) return null
  return section
}

/** One direction of a row's swipe, or none when the setting says nothing. */
function swipeSide(
  choice: SwipeChoice,
  convo: ConversationSummary,
  onContextAction: (threadId: string, action: SingleAction) => void
): null | SwipeSide {
  const action = swipeAction(choice, convo)
  if (action === null) return null
  return { action, run: () => onContextAction(convo.thread_id, action) }
}
