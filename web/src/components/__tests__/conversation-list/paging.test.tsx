import type { ConversationSummary } from '@/lib/types'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render } from '@testing-library/react'
import { Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { LOAD_AHEAD_ROWS, nearEnd } from '@/lib/list-paging'

// jsdom lays nothing out, so the virtualizer is told which rows are on
// screen: `visible.end` is the last visible index
const visible = { end: -1 }
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    range: { endIndex: visible.end, startIndex: 0 },
    getTotalSize: () => count * 64,
    getVirtualItems: () =>
      Array.from({ length: Math.min(count, 12) }, (_, i) => ({ index: i, start: i * 64 })),
  }),
}))

const { VirtualConversationList } = await import('@/components/conversation-list-virtual')

function conversations(n: number): ConversationSummary[] {
  return Array.from(
    { length: n },
    (_, i) =>
      ({
        archived: false,
        category: 'general',
        flagged: false,
        last_date: 1_700_000_000 - i,
        participants: ['a@example.com'],
        pinned: false,
        snippet: '',
        subject: `s${i}`,
        thread_id: `t${i}`,
        unread_count: 0,
      }) as unknown as ConversationSummary
  )
}

function renderList(opts: { hasMore: boolean; loadingMore?: boolean; onNearEnd: () => void }) {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <Provider>
        <VirtualConversationList
          batchMode={false}
          conversations={conversations(50)}
          dateLabel={() => 'Today'}
          error={null}
          folder={null}
          hasBatchBar={false}
          hasMore={opts.hasMore}
          initialLoading={false}
          isError={false}
          isSearching={false}
          loadingMore={opts.loadingMore ?? false}
          myEmail="me@example.com"
          onContextAction={() => {}}
          onNearEnd={opts.onNearEnd}
          onSelect={() => {}}
          onToggleCheck={() => {}}
          scrollContainerRef={{ current: null }}
          selectedId={null}
          selectedThreadIds={new Set()}
        />
      </Provider>
    </QueryClientProvider>
  )
}

afterEach(() => {
  cleanup()
  visible.end = -1
})

describe('the next page is asked for before the end of the rows', () => {
  it('counts from the last visible item', () => {
    expect(nearEnd(-1, 10)).toBe(false)
    expect(nearEnd(0, 100)).toBe(false)
    expect(nearEnd(99 - LOAD_AHEAD_ROWS - 1, 100)).toBe(false)
    expect(nearEnd(99 - LOAD_AHEAD_ROWS, 100)).toBe(true)
    expect(nearEnd(99, 100)).toBe(true)
  })

  // 50 rows, one date divider and the sentinel: 52 items
  it('asks while there are still rows below the screen', () => {
    const onNearEnd = vi.fn()
    visible.end = 51 - LOAD_AHEAD_ROWS
    renderList({ hasMore: true, onNearEnd })
    expect(onNearEnd).toHaveBeenCalled()
  })

  it('does not ask from the top of a long page', () => {
    const onNearEnd = vi.fn()
    visible.end = 11
    renderList({ hasMore: true, onNearEnd })
    expect(onNearEnd).not.toHaveBeenCalled()
  })

  it('does not ask when there is no next page or one is on its way', () => {
    const onNearEnd = vi.fn()
    visible.end = 51
    renderList({ hasMore: false, onNearEnd })
    cleanup()
    renderList({ hasMore: true, loadingMore: true, onNearEnd })
    expect(onNearEnd).not.toHaveBeenCalled()
  })
})
