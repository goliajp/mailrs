// A swipe on a row runs what the user's swipe setting says: right archives
// and left moves to the Trash until they choose otherwise, and in the Trash
// a swipe restores rather than deleting.

import type { SingleAction } from '@/components/conversation-actions'
import type { ConversationSummary } from '@/lib/types'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    range: { endIndex: 0, startIndex: 0 },
    getTotalSize: () => count * 64,
    getVirtualItems: () => Array.from({ length: count }, (_, i) => ({ index: i, start: i * 64 })),
  }),
}))
vi.mock('@/hooks/use-is-mobile', () => ({ useIsMobile: () => true }))
vi.mock('@/wire/endpoints/mail-settings', () => ({
  wireGetSwipe: vi.fn(),
  wireSetSwipe: vi.fn(),
}))

const { VirtualConversationList } = await import('@/components/conversation-list-virtual')
const { wireGetSwipe } = await import('@/wire/endpoints/mail-settings')

function row(over: Partial<ConversationSummary>): ConversationSummary {
  return {
    archived: false,
    category: 'general',
    flagged: false,
    last_date: 1_700_000_000,
    participants: ['a@example.com'],
    pinned: false,
    snippet: '',
    subject: 'Quarterly numbers',
    thread_id: 't1',
    unread_count: 1,
    ...over,
  } as unknown as ConversationSummary
}

function show(convo: ConversationSummary, onContextAction: (id: string, a: SingleAction) => void) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Provider>
        <VirtualConversationList
          batchMode={false}
          conversations={[convo]}
          dateLabel={() => 'Today'}
          error={null}
          folder={null}
          hasBatchBar={false}
          hasMore={false}
          initialLoading={false}
          isError={false}
          isSearching={false}
          loadingMore={false}
          myEmail="me@example.com"
          onContextAction={onContextAction}
          onNearEnd={() => {}}
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

/** A finger from x=200 moving `dx` across the subject line. */
async function swipe(dx: number) {
  const target = await screen.findByText('Quarterly numbers')
  fireEvent.touchStart(target, { touches: [{ clientX: 200, clientY: 100 }] })
  fireEvent.touchMove(target, { touches: [{ clientX: 200 + dx / 2, clientY: 100 }] })
  fireEvent.touchMove(target, { touches: [{ clientX: 200 + dx, clientY: 100 }] })
  fireEvent.touchEnd(target)
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('swiping a row', () => {
  it('archives to the right and moves to the Trash to the left by default', async () => {
    vi.mocked(wireGetSwipe).mockResolvedValue({ left: 'trash', right: 'archive' })
    const acted = vi.fn()
    show(row({}), acted)
    await swipe(110)
    expect(acted).toHaveBeenLastCalledWith('t1', 'archive')
    await swipe(-110)
    expect(acted).toHaveBeenLastCalledWith('t1', 'trash')
  })

  it('does what the setting says once it has loaded', async () => {
    vi.mocked(wireGetSwipe).mockResolvedValue({ left: 'none', right: 'read' })
    const acted = vi.fn()
    show(row({}), acted)
    await vi.waitFor(() => expect(wireGetSwipe).toHaveBeenCalled())
    await vi.waitFor(async () => {
      acted.mockClear()
      await swipe(110)
      expect(acted).toHaveBeenLastCalledWith('t1', 'read')
    })
    acted.mockClear()
    await swipe(-110)
    expect(acted).not.toHaveBeenCalled()
  })

  it('restores in the Trash, never deletes', async () => {
    vi.mocked(wireGetSwipe).mockResolvedValue({ left: 'trash', right: 'archive' })
    const acted = vi.fn()
    show(row({ trashed_at: 1_700_000_100 }), acted)
    await swipe(-110)
    expect(acted).toHaveBeenLastCalledWith('t1', 'untrash')
    await swipe(110)
    expect(acted).toHaveBeenLastCalledWith('t1', 'untrash')
    expect(acted).not.toHaveBeenCalledWith('t1', 'delete-forever')
  })
})
