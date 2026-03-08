import { Provider, createStore } from 'jotai'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'

import type { ConversationSummary } from '@/lib/types'
import { authAtom } from '@/store/auth'
import {
  batchModeAtom,
  conversationsAtom,
  hasMoreAtom,
  initialLoadingAtom,
  selectedThreadIdsAtom,
} from '@/store/chat'

// mock api module
vi.mock('@/lib/api', () => ({
  fetchJson: vi.fn(() => Promise.resolve([])),
  postJson: vi.fn(() => Promise.resolve({ success: true })),
}))

// mock sonner toast
vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

// mock localStorage
function makeLocalStorageMock(): Storage {
  const store: Record<string, string> = {}
  return {
    getItem: (k: string) => store[k] ?? null,
    setItem: (k: string, v: string) => { store[k] = v },
    removeItem: (k: string) => { delete store[k] },
    clear: () => { for (const k in store) delete store[k] },
    key: (n: number) => Object.keys(store)[n] ?? null,
    get length() { return Object.keys(store).length },
  } as Storage
}
vi.stubGlobal('localStorage', makeLocalStorageMock())

// mock IntersectionObserver
const mockIntersectionObserver = vi.fn()
mockIntersectionObserver.mockReturnValue({
  observe: vi.fn(),
  unobserve: vi.fn(),
  disconnect: vi.fn(),
})
vi.stubGlobal('IntersectionObserver', mockIntersectionObserver)

function makeConversation(overrides: Partial<ConversationSummary> = {}): ConversationSummary {
  return {
    thread_id: 'thread-1',
    subject: 'Test Subject',
    participants: ['alice@example.com'],
    message_count: 1,
    unread_count: 0,
    last_date: Math.floor(Date.now() / 1000),
    category: 'general',
    flagged: false,
    snippet: 'A snippet',
    pinned: false,
    archived: false,
    importance_level: 'normal',
    importance_score: 0.3,
    ...overrides,
  }
}

function makeStore() {
  const store = createStore()
  store.set(authAtom, {
    token: 'test-token',
    address: 'user@example.com',
    display_name: 'Test User',
    super_domains: ['example.com', 'golia.jp'],
  })
  store.set(initialLoadingAtom, false)
  return store
}

function Wrapper({ store, children }: { store: ReturnType<typeof createStore>; children: ReactNode }) {
  return <Provider store={store}>{children}</Provider>
}

// must import after mocks
const { ConversationList } = await import('@/components/conversation-list')

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('DomainSelector', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
  })

  it('renders domain buttons when super_domains are available', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByLabelText('Domain filter')).toBeDefined()
    expect(screen.getByLabelText('Filter by domain example.com')).toBeDefined()
    expect(screen.getByLabelText('Filter by domain golia.jp')).toBeDefined()
  })

  it('shows Mine button initially (no domains selected)', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('Mine')).toBeDefined()
  })

  it('toggles domain selection on click', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    const domainButton = screen.getByLabelText('Filter by domain example.com')
    fireEvent.click(domainButton)
    // after clicking one domain, Mine changes to Mix
    expect(screen.getByText('Mix')).toBeDefined()
  })

  it('shows All when all domains are selected', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    fireEvent.click(screen.getByLabelText('Filter by domain example.com'))
    fireEvent.click(screen.getByLabelText('Filter by domain golia.jp'))
    expect(screen.getAllByText('All').length).toBeGreaterThanOrEqual(1)
  })

  it('does not render when super_domains is empty', () => {
    const noSuperStore = createStore()
    noSuperStore.set(authAtom, {
      token: 'test-token',
      address: 'user@example.com',
      display_name: 'Test User',
      super_domains: [],
    })
    noSuperStore.set(initialLoadingAtom, false)
    noSuperStore.set(conversationsAtom, [makeConversation()])

    render(
      <Wrapper store={noSuperStore}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.queryByLabelText('Domain filter')).toBeNull()
  })
})

describe('SortSelector', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
  })

  it('renders all sort options', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByLabelText('Sort order')).toBeDefined()
    expect(screen.getByLabelText('Sort by Newest')).toBeDefined()
    expect(screen.getByLabelText('Sort by Oldest')).toBeDefined()
    expect(screen.getByLabelText('Sort by Unread first')).toBeDefined()
  })

  it('defaults to newest being pressed', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    const newestBtn = screen.getByLabelText('Sort by Newest')
    expect(newestBtn.getAttribute('aria-pressed')).toBe('true')
  })

  it('updates sort order on click', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    fireEvent.click(screen.getByLabelText('Sort by Oldest'))
    expect(screen.getByLabelText('Sort by Oldest').getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByLabelText('Sort by Newest').getAttribute('aria-pressed')).toBe('false')
  })

  it('applies oldest sort to conversations', () => {
    const now = Math.floor(Date.now() / 1000)
    store.set(conversationsAtom, [
      makeConversation({ thread_id: 'newer', last_date: now, subject: 'Newer' }),
      makeConversation({ thread_id: 'older', last_date: now - 86400, subject: 'Older' }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    fireEvent.click(screen.getByLabelText('Sort by Oldest'))

    const items = screen.getAllByRole('listitem')
    // oldest first
    expect(items[0].textContent).toContain('Older')
    expect(items[1].textContent).toContain('Newer')
  })
})

describe('ArchivedToggle', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
  })

  it('renders the Active button in unpressed state by default', () => {
    store.set(conversationsAtom, [makeConversation()])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    const btn = screen.getByText('Active')
    expect(btn.getAttribute('aria-pressed')).toBe('false')
  })

  it('toggles to Archived on click', () => {
    store.set(conversationsAtom, [makeConversation()])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    const btn = screen.getByText('Active')
    fireEvent.click(btn)
    // after toggle, button text changes to "Archived" with aria-pressed=true
    const archived = screen.getByText('Archived')
    expect(archived.getAttribute('aria-pressed')).toBe('true')
  })

  it('shows archived conversations when toggled on', () => {
    store.set(conversationsAtom, [
      makeConversation({ thread_id: 'normal', subject: 'Normal Item', archived: false }),
      makeConversation({ thread_id: 'archived', subject: 'Archived Item', archived: true }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    // archived item hidden by default
    expect(screen.queryByText('Archived Item')).toBeNull()

    // toggle on
    fireEvent.click(screen.getByText('Active'))
    expect(screen.getByText('Archived Item')).toBeDefined()
  })
})

describe('BatchActionBar', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    store.set(conversationsAtom, [
      makeConversation({ thread_id: 't1', subject: 'Thread 1' }),
      makeConversation({ thread_id: 't2', subject: 'Thread 2' }),
    ])
  })

  it('does not show batch action bar initially', () => {
    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.queryByText(/selected$/)).toBeNull()
  })

  it('shows batch action bar when batch mode active with selections', () => {
    store.set(batchModeAtom, true)
    store.set(selectedThreadIdsAtom, new Set(['t1']))

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
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
      </Wrapper>,
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
      </Wrapper>,
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
      </Wrapper>,
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
    store.set(conversationsAtom, [])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('All caught up!')).toBeDefined()
    expect(screen.getByText('No conversations to show')).toBeDefined()
  })

  it('shows search-specific empty state during search', () => {
    store.set(conversationsAtom, [])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    const searchInput = screen.getByLabelText('Search conversations')
    fireEvent.change(searchInput, { target: { value: 'nonexistent' } })

    expect(screen.getByText('No results found')).toBeDefined()
    expect(screen.getByText('Try a different search term')).toBeDefined()
  })

  it('shows "No more conversations" when hasMore is false', () => {
    store.set(conversationsAtom, [makeConversation()])
    store.set(hasMoreAtom, false)

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
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
    store.set(conversationsAtom, [
      makeConversation({ subject: 'Important Email', participants: ['Alice Smith <alice@example.com>'] }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('Important Email')).toBeDefined()
    expect(screen.getByText('Alice Smith')).toBeDefined()
  })

  it('shows unread count badge', () => {
    store.set(conversationsAtom, [
      makeConversation({ unread_count: 5 }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('5')).toBeDefined()
  })

  it('shows (no subject) for empty subject', () => {
    store.set(conversationsAtom, [
      makeConversation({ subject: '' }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('(no subject)')).toBeDefined()
  })

  it('shows participant count when multiple participants', () => {
    store.set(conversationsAtom, [
      makeConversation({
        participants: ['alice@example.com', 'bob@example.com', 'charlie@example.com'],
      }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('+2')).toBeDefined()
  })

  it('shows snippet when available', () => {
    store.set(conversationsAtom, [
      makeConversation({ snippet: 'This is a preview...' }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('This is a preview...')).toBeDefined()
  })

  it('shows category badge for non-general categories', () => {
    store.set(conversationsAtom, [
      makeConversation({ category: 'newsletter' }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.getByText('Newsletter')).toBeDefined()
  })

  it('does not show category badge for general category', () => {
    store.set(conversationsAtom, [
      makeConversation({ category: 'general' }),
    ])

    render(
      <Wrapper store={store}>
        <ConversationList onLoadMore={vi.fn()} />
      </Wrapper>,
    )

    expect(screen.queryByText('General')).toBeNull()
  })
})
