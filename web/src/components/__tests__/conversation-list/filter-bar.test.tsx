import type { ConversationSummary } from '@/lib/types'
import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The batch wire call, so a test can say whether it was reached.
const batchCalls: Array<{ action: string; ids: string[] }> = []
vi.mock('@/wire/endpoints/mutations', () => ({
  wireBatchMutation: (action: string, ids: string[]) => {
    batchCalls.push({ action, ids })
    return Promise.resolve({ failed: 0, failed_thread_ids: [], message: 'Done', success: true })
  },
  wireMarkAllRead: () => Promise.resolve({ flipped: 0 }),
}))

import { MAIL_LIST_TABS, MAIL_LISTS } from '@/lib/mail-lists'
import { authAtom } from '@/store/auth'

// v2.1 phase-5d: the mail-list conversations shape lives entirely in
// React Query in production. Tests hoist a mutable stub and mock
// `useFlatConversations` to read it — same idea as seeding an atom,
// no jotai plumbing. Set fields before calling `render(...)`.
const flatStub: {
  conversations: ConversationSummary[]
  hasMore: boolean
  initialLoading: boolean
  loadingMore: boolean
} = {
  conversations: [],
  hasMore: true,
  initialLoading: false,
  loadingMore: false,
}

// mock api module
vi.mock('@/lib/api', () => ({
  // `useCurrentListRows` calls every source hook so the hook order is
  // fixed as the list changes; the draft one reaches lib/api even while
  // disabled.
  deleteDraft: vi.fn(() => Promise.resolve({ success: true })),
  fetchJson: vi.fn(() => Promise.resolve([])),
  listDrafts: vi.fn(() => Promise.resolve([])),
  postJson: vi.fn(() => Promise.resolve({ success: true })),
}))

// mock react-query hooks used by ConversationList. The real ones need a
// QueryClientProvider in the tree, which is overkill for the component
// shape tests below.
vi.mock('@/hooks/use-mail-queries', () => ({
  useActionCountQuery: () => ({ data: { count: 0 } }),
  useCategoriesQuery: () => ({ data: [] }),
  useConversationsQuery: () => ({
    data: { pageParams: [], pages: [] },
    hasNextPage: false,
    isFetchingNextPage: false,
    isPending: false,
  }),
  // The Review tab's count. Zero here so the tab renders as it did
  // before the count existed; the test that asserts the number shows
  // supplies its own.
  useQuarantineCountQuery: () => ({ data: heldCountStub.value }),
}))

// Mutable so one test can give the Review tab a number without every
// other test rendering one. `vi.mock` is hoisted, so this has to be a
// reference the factory closes over rather than a value it captures.
const heldCountStub = { value: 0 }
// Mutable, because switching lists is a change of this value and the
// component does not remount when it happens — which is exactly what the
// scroll and selection bugs were.
const filtersStub: { current: Record<string, unknown> } = { current: {} }
vi.mock('@/hooks/use-current-mail-filters', () => ({
  useCurrentMailFilters: () => filtersStub.current,
}))
vi.mock('@/hooks/use-flat-conversations', () => ({
  useFlatConversations: () => flatStub,
}))
const stubMutation = () => ({ isPending: false, mutate: vi.fn(), mutateAsync: vi.fn() })
vi.mock('@/hooks/use-mail-mutations', () => ({
  useArchiveMutation: stubMutation,
  useDeleteMutation: stubMutation,
  useMarkJunkMutation: stubMutation,
  useMarkNotificationMutation: stubMutation,
  useMarkNotJunkMutation: stubMutation,
  useMarkPromotionMutation: stubMutation,
  useMarkReadMutation: stubMutation,
  useMarkUnreadMutation: stubMutation,
  useMoveToInboxMutation: stubMutation,
  usePinMutation: stubMutation,
  useSnoozeMutation: stubMutation,
  useStarMutation: stubMutation,
  useUnarchiveMutation: stubMutation,
  useUnpinMutation: stubMutation,
  useUnsnoozeMutation: stubMutation,
  useUnstarMutation: stubMutation,
}))

// mock sonner toast
vi.mock('sonner', () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}))

// mock virtualizer — jsdom has zero-height elements so virtualizer won't render
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 72,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, i) => ({
        index: i,
        key: i,
        size: 72,
        start: i * 72,
        measureElement: () => {},
      })),
    measureElement: () => {},
  }),
}))

// mock localStorage
function makeLocalStorageMock(): Storage {
  const store: Record<string, string> = {}
  return {
    clear: () => {
      for (const k in store) delete store[k]
    },
    getItem: (k: string) => store[k] ?? null,
    key: (n: number) => Object.keys(store)[n] ?? null,
    get length() {
      return Object.keys(store).length
    },
    removeItem: (k: string) => {
      delete store[k]
    },
    setItem: (k: string, v: string) => {
      store[k] = v
    },
  } as Storage
}
vi.stubGlobal('localStorage', makeLocalStorageMock())

// mock IntersectionObserver
const mockIntersectionObserver = vi.fn().mockImplementation(
  class {
    disconnect = vi.fn()
    observe = vi.fn()
    unobserve = vi.fn()
  } as any
)
vi.stubGlobal('IntersectionObserver', mockIntersectionObserver)

function makeConversation(overrides: Partial<ConversationSummary> = {}): ConversationSummary {
  return {
    archived: false,
    category: 'general',
    flagged: false,
    importance_level: 'normal',
    importance_score: 0.3,
    last_date: Math.floor(Date.now() / 1000),
    message_count: 1,
    participants: ['alice@example.com'],
    pinned: false,
    received_count: 1,
    requires_action: false,
    sent_count: 0,
    snippet: 'A snippet',
    subject: 'Test Subject',
    thread_id: 'thread-1',
    unread_count: 0,
    ...overrides,
  }
}

function makeStore() {
  const store = createStore()
  store.set(authAtom, {
    accessible_domains: ['example.com', 'golia.jp'],
    address: 'user@example.com',
    display_name: 'Test User',
    permissions: [],
    token: 'test-token',
  })
  flatStub.initialLoading = false
  return store
}

// `useCurrentListRows` calls every source hook so the hook order stays
// fixed as the list changes (only one of them is enabled), which means a
// QueryClient has to be in scope even for a test that mocks the
// conversation query.
const testQueryClient = new QueryClient({
  defaultOptions: { queries: { gcTime: 0, retry: false } },
})

function Wrapper({
  children,
  store,
}: {
  children: ReactNode
  store: ReturnType<typeof createStore>
}) {
  return (
    <QueryClientProvider client={testQueryClient}>
      <Provider store={store}>{children}</Provider>
    </QueryClientProvider>
  )
}

// must import after mocks
const { ConversationList } = await import('@/components/conversation-list')

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

// helper: open the filter dropdown panel
function openFilterPanel() {
  fireEvent.click(screen.getByLabelText('Toggle filters'))
}

// domain selector tests removed — domains moved to sidebar

describe('FilterBar — sort', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    flatStub.conversations = [makeConversation()]
  })

  it('shows sort options in filter dropdown', () => {
    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    openFilterPanel()
    expect(screen.getByText('Sort')).toBeDefined()
    expect(screen.getByText('newest')).toBeDefined()
    expect(screen.getByText('oldest')).toBeDefined()
    expect(screen.getByText('Unread first')).toBeDefined()
  })

  it('applies oldest sort to conversations', () => {
    const now = Math.floor(Date.now() / 1000)
    flatStub.conversations = [
      makeConversation({
        last_date: now,
        subject: 'Newer',
        thread_id: 'newer',
      }),
      makeConversation({
        last_date: now - 86400,
        subject: 'Older',
        thread_id: 'older',
      }),
    ]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    openFilterPanel()
    fireEvent.click(screen.getByText('oldest'))

    const items = screen.getAllByRole('listitem')
    expect(items[0].textContent).toContain('Older')
    expect(items[1].textContent).toContain('Newer')
  })
})

describe('FilterBar — archived tab', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
  })

  // v2.8.2 — Archived moved from the advanced-filter panel toggle to
  // a first-class view tab; "All" became "Inbox" (user directive
  // 2026-07-14).
  it('shows Inbox and Archived as view tabs', () => {
    flatStub.conversations = [makeConversation()]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('Inbox')).toBeDefined()
    expect(screen.getByText('Archived')).toBeDefined()
  })

  /**
   * **The Review tab carries no number.**
   *
   * A count of what was held is the held mail advertising itself: the
   * screen exists so an attempt to defraud stops competing for
   * attention, and `Review 443` puts it back on the tab. Held mail is
   * marked read for the same reason.
   *
   * The count is not gone — `/api/quarantine` returns a `total`, and
   * the route is no longer capped below the real figure. It is simply
   * not something the reader is shown unprompted.
   */
  it('puts no number on the Review tab', () => {
    flatStub.conversations = [makeConversation()]
    heldCountStub.value = 443

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('Review')).toBeDefined()
    expect(screen.queryByText('443')).toBeNull()
    heldCountStub.value = 0
  })

  /**
   * Every tab is the same width, and the second row lines up under the
   * first.
   *
   * They were two wrapped flex rows, so each tab was as wide as its own
   * label: `Inbox` narrow, `Archived` wide, and the row below starting
   * under nothing in particular. Asserted on the grid rather than on
   * measured pixels — jsdom lays nothing out, so a width assertion here
   * would pass whatever the classes said.
   */
  /**
   * A held conversation is on screen only because the reader asked to
   * see held ones. Unmarked, it is indistinguishable from ordinary
   * mail — which is worse than hiding it, because the reader would
   * have no way to tell the message judged an attempt to defraud them
   * from the rest.
   */
  it('marks a held conversation, and marks nothing else', () => {
    flatStub.conversations = [
      makeConversation({ subject: 'Ordinary', thread_id: 't1' }),
      makeConversation({ quarantined: true, subject: 'Held', thread_id: 't2' }),
    ]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    // One mark, on the held row and not the other.
    const marks = screen.getAllByLabelText('Held: suspected fraud')
    expect(marks).toHaveLength(1)
    expect(marks[0]?.closest('[role="option"], li, div')).not.toBeNull()
    expect(screen.getByText('Ordinary')).toBeTruthy()
  })

  it('lays the tabs out as one five-column grid', () => {
    flatStub.conversations = [makeConversation()]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    const inbox = screen.getByText('Inbox')
    const grid = inbox.parentElement
    expect(grid?.className).toContain('grid-cols-5')
    // Every tab in one container, not the overflow in a second row of
    // its own. Derived from the registry rather than counted: the
    // number was `8` until a ninth tab was added, and a test that
    // pins a count is asserting how many lists exist, which is not
    // what this one is about.
    expect(grid?.children.length).toBe(MAIL_LIST_TABS.length)
    for (const id of MAIL_LIST_TABS) {
      const tab = screen.getByText(MAIL_LISTS[id].label)
      expect(tab.className, `${MAIL_LISTS[id].label} does not fill its column`).toContain('w-full')
    }
  })

  /**
   * The tab asks the server for the archived axis; it does not sift the
   * page it was given.
   *
   * This used to assert that an archived row disappeared from the Inbox,
   * which the client did by filtering the page after the fact — from a
   * page whose size the server had already reported, so the count and
   * the rows disagreed. Since 2026-08-05 the server excludes archived
   * threads from every list but this one, and what is left for the
   * client to get right is which list it asks for.
   */
  it('switches to the archived axis when the Archived tab is clicked', async () => {
    const { activeListAtom, showArchivedAtom } = await import('@/store/ui')
    flatStub.conversations = [makeConversation()]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )
    expect(store.get(showArchivedAtom)).toBe(false)

    fireEvent.click(screen.getByText('Archived'))

    expect(store.get(activeListAtom)).toBe('archived')
    expect(store.get(showArchivedAtom)).toBe(true)
  })
})
