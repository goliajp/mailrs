import type { ConversationSummary } from '@/lib/types'
import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
}))
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

// domain selector tests removed — domains moved to sidebar

describe('ConversationList empty states', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
  })

  it('shows empty state when no conversations', () => {
    flatStub.conversations = []

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('All caught up!')).toBeDefined()
  })

  it('shows search-specific empty state during search', () => {
    flatStub.conversations = []

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    const searchInput = screen.getByLabelText('Search conversations')
    fireEvent.change(searchInput, { target: { value: 'nonexistent' } })

    expect(screen.getByText('No results found')).toBeDefined()
    expect(screen.getByText('Try a different search term')).toBeDefined()
  })

  it('shows "No more conversations" when hasMore is false', () => {
    flatStub.conversations = [makeConversation()]
    flatStub.hasMore = false

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('No more conversations')).toBeDefined()
  })
})

describe('ConversationItem rendering', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
  })

  it('renders subject and participant name', () => {
    flatStub.conversations = [
      makeConversation({
        participants: ['Alice Smith <alice@example.com>'],
        subject: 'Important Email',
      }),
    ]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('Important Email')).toBeDefined()
    expect(screen.getByText('Alice Smith')).toBeDefined()
  })

  it('shows unread count badge', () => {
    flatStub.conversations = [makeConversation({ unread_count: 5 })]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('5')).toBeDefined()
  })

  it('shows (no subject) for empty subject', () => {
    flatStub.conversations = [makeConversation({ subject: '' })]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('(no subject)')).toBeDefined()
  })

  it('shows participant count when multiple participants', () => {
    flatStub.conversations = [
      makeConversation({
        participants: ['alice@example.com', 'bob@example.com', 'charlie@example.com'],
      }),
    ]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('+2')).toBeDefined()
  })

  it('does not render the snippet line (compact rows, 2026-07-17)', () => {
    flatStub.conversations = [makeConversation({ snippet: 'This is a preview...' })]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.queryByText('This is a preview...')).toBeNull()
  })

  it('shows category badge for non-general categories', () => {
    flatStub.conversations = [makeConversation({ category: 'newsletter' })]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByText('Newsletter')).toBeDefined()
  })

  it('does not show category badge for general category', () => {
    flatStub.conversations = [makeConversation({ category: 'general' })]

    render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.queryByText('General')).toBeNull()
  })
})

describe('switching lists', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    filtersStub.current = { folder: 'Inbox' }
    sessionStorage.clear()
  })

  /**
   * The component does not remount when the list changes, so scroll and
   * selection both carried over: scrolling the Inbox and opening Junk showed
   * the middle of Junk, and a thread selected in the Inbox stayed open above
   * the Junk list.
   */
  it('selects the first message of the list it switched to', async () => {
    const { activeListAtom, pickedItemAtom } = await import('@/store/ui')
    flatStub.conversations = [makeConversation({ subject: 'inbox one', thread_id: 'inbox-1' })]

    const view = render(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )
    // `aria-current`, not `aria-selected`: the row's activation is a
    // button, and selected is only defined on option / tab / row /
    // gridcell — it was being ignored by assistive tech here.
    expect(screen.getByLabelText(/inbox one/).getAttribute('aria-current')).toBe('true')

    // Pick the row explicitly, then switch lists. The pick has to be
    // dropped by the switch, or it survives into a list it is not in —
    // a thread you replied in is in both Inbox and Send.
    store.set(pickedItemAtom, { list: 'inbox', threadId: 'inbox-1', uid: null })
    store.set(activeListAtom, 'junk')
    filtersStub.current = { folder: 'Junk' }
    flatStub.conversations = [makeConversation({ subject: 'junk one', thread_id: 'junk-1' })]
    view.rerender(
      <Wrapper store={store}>
        <ConversationList />
      </Wrapper>
    )

    expect(screen.getByLabelText(/junk one/).getAttribute('aria-current')).toBe('true')
  })

  /**
   * The saved scroll position used to be one module variable and one fixed
   * sessionStorage key for every list. Each list keeps its own now, and
   * arriving at one puts it at the top.
   */
  it('keeps each list its own scroll position', async () => {
    const { listIdentity } = await import('@/lib/list-identity')
    const inbox = listIdentity({ folder: 'Inbox' } as never)
    const sent = listIdentity({ folder: 'Sent' } as never)

    expect(inbox).not.toBe(sent)

    // A position saved against the Inbox must not be readable as Sent's.
    sessionStorage.setItem(`chat:list-scroll:${inbox}`, '420')
    expect(sessionStorage.getItem(`chat:list-scroll:${sent}`)).toBeNull()
  })

  /** The same filters are the same list, however the object was built. */
  it('gives one list one identity', async () => {
    const { listIdentity } = await import('@/lib/list-identity')
    expect(listIdentity({ folder: 'Inbox', unread: true } as never)).toBe(
      listIdentity({ folder: 'Inbox', unread: true } as never)
    )
    expect(listIdentity({ folder: 'Inbox', unread: true } as never)).not.toBe(
      listIdentity({ folder: 'Inbox', unread: false } as never)
    )
  })
})
