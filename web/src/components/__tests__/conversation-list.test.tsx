import type { ConversationSummary } from '@/lib/types'
import type { ReactNode } from 'react'

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { authAtom } from '@/store/auth'
import { batchModeAtom, selectedThreadIdsAtom } from '@/store/chat'

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
  fetchJson: vi.fn(() => Promise.resolve([])),
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
vi.mock('@/hooks/use-current-mail-filters', () => ({
  useCurrentMailFilters: () => ({}),
}))
vi.mock('@/hooks/use-flat-conversations', () => ({
  useFlatConversations: () => flatStub,
}))
const stubMutation = () => ({ isPending: false, mutate: vi.fn(), mutateAsync: vi.fn() })
vi.mock('@/hooks/use-mail-mutations', () => ({
  useArchiveMutation: stubMutation,
  useDeleteMutation: stubMutation,
  useMarkReadMutation: stubMutation,
  useMarkUnreadMutation: stubMutation,
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
    last_sender: 'alice@example.com',
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

function Wrapper({
  children,
  store,
}: {
  children: ReactNode
  store: ReturnType<typeof createStore>
}) {
  return <Provider store={store}>{children}</Provider>
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
        <ConversationList onLoadMore={vi.fn()} />
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
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    openFilterPanel()
    fireEvent.click(screen.getByText('oldest'))

    const items = screen.getAllByRole('listitem')
    expect(items[0].textContent).toContain('Older')
    expect(items[1].textContent).toContain('Newer')
  })
})

describe('FilterBar — archived toggle', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
  })

  it('shows Active/Archived in filter dropdown', () => {
    flatStub.conversations = [makeConversation()]

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    openFilterPanel()
    expect(screen.getByText('Active')).toBeDefined()
    expect(screen.getByText('Archived')).toBeDefined()
  })

  it('shows archived conversations when Archived clicked', () => {
    flatStub.conversations = [
      makeConversation({
        archived: false,
        subject: 'Normal Item',
        thread_id: 'normal',
      }),
      makeConversation({
        archived: true,
        subject: 'Archived Item',
        thread_id: 'archived',
      }),
    ]

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.queryByText('Archived Item')).toBeNull()

    openFilterPanel()
    fireEvent.click(screen.getByText('Archived'))
    expect(screen.getByText('Archived Item')).toBeDefined()
  })
})

describe('BatchActionBar', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    flatStub.conversations = [
      makeConversation({ subject: 'Thread 1', thread_id: 't1' }),
      makeConversation({ subject: 'Thread 2', thread_id: 't2' }),
    ]
  })

  it('does not show batch action bar initially', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.queryByText(/selected$/)).toBeNull()
  })

  it('shows batch action bar when batch mode active with selections', () => {
    store.set(batchModeAtom, true)
    store.set(selectedThreadIdsAtom, new Set(['t1']))

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('1 selected')).toBeDefined()
    expect(screen.getByText('Mark read')).toBeDefined()
    expect(screen.getByText('Mark unread')).toBeDefined()
    expect(screen.getByText('Star')).toBeDefined()
    expect(screen.getByText('Archive')).toBeDefined()
    expect(screen.getByText('Delete')).toBeDefined()
    expect(screen.getByText('Cancel')).toBeDefined()
  })

  it('toggles batch mode via batch select button', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    const batchButton = screen.getByLabelText('Enter batch select mode')
    fireEvent.click(batchButton)

    // now clicking a conversation should check it instead of selecting
    const firstItem = screen.getAllByRole('listitem')[0]
    fireEvent.click(firstItem.querySelector('button')!)
    // batch action bar should appear
    expect(screen.getByText('1 selected')).toBeDefined()
  })

  it('exits batch mode via cancel button', () => {
    store.set(batchModeAtom, true)
    store.set(selectedThreadIdsAtom, new Set(['t1']))

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.queryByText(/selected$/)).toBeNull()
  })

  it('shows correct count when multiple items selected', () => {
    store.set(batchModeAtom, true)
    store.set(selectedThreadIdsAtom, new Set(['t1', 't2']))

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('2 selected')).toBeDefined()
  })
})

describe('ConversationList empty states', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
  })

  it('shows empty state when no conversations', () => {
    flatStub.conversations = []

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('All caught up!')).toBeDefined()
  })

  it('shows search-specific empty state during search', () => {
    flatStub.conversations = []

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
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
        <ConversationList onLoadMore={vi.fn()} />
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
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('Important Email')).toBeDefined()
    expect(screen.getByText('Alice Smith')).toBeDefined()
  })

  it('shows unread count badge', () => {
    flatStub.conversations = [makeConversation({ unread_count: 5 })]

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('5')).toBeDefined()
  })

  it('shows (no subject) for empty subject', () => {
    flatStub.conversations = [makeConversation({ subject: '' })]

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
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
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('+2')).toBeDefined()
  })

  it('shows snippet when available', () => {
    flatStub.conversations = [makeConversation({ snippet: 'This is a preview...' })]

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('This is a preview...')).toBeDefined()
  })

  it('shows category badge for non-general categories', () => {
    flatStub.conversations = [makeConversation({ category: 'newsletter' })]

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.getByText('Newsletter')).toBeDefined()
  })

  it('does not show category badge for general category', () => {
    flatStub.conversations = [makeConversation({ category: 'general' })]

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>
    )

    expect(screen.queryByText('General')).toBeNull()
  })
})
