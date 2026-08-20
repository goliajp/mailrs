import type { ConversationSummary, ThreadMessage } from '@/lib/types'
import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { makeConversation, makeMessage } from './thread-view-fixtures'

const localStorageStore: Record<string, string> = {}
vi.stubGlobal('localStorage', {
  clear: vi.fn(() => {
    Object.keys(localStorageStore).forEach((k) => delete localStorageStore[k])
  }),
  getItem: vi.fn((key: string) => localStorageStore[key] ?? null),
  key: vi.fn(() => null),
  length: 0,
  removeItem: vi.fn((key: string) => {
    delete localStorageStore[key]
  }),
  setItem: vi.fn((key: string, value: string) => {
    localStorageStore[key] = value
  }),
})

import { authAtom } from '@/store/auth'

// v2.1 phase-5d: production reads the conversations list through
// React Query; tests write to this mutable stub and the mocked
// `useFlatConversations` (below) reads it.
const flatStub: {
  conversations: ConversationSummary[]
  hasMore: boolean
  initialLoading: boolean
  loadingMore: boolean
} = {
  conversations: [],
  hasMore: false,
  initialLoading: false,
  loadingMore: false,
}

vi.mock('@/lib/api', () => ({
  // `useCurrentListRows` calls every source hook so the hook order is
  // fixed as the list changes; the draft one reaches lib/api even while
  // disabled.
  deleteDraft: vi.fn(() => Promise.resolve({ success: true })),
  deleteJson: vi.fn(() => Promise.resolve({ success: true })),
  fetchJson: vi.fn(() => Promise.resolve([])),
  getThreadReactions: vi.fn(() => Promise.resolve({})),
  listDrafts: vi.fn(() => Promise.resolve([])),
  postJson: vi.fn(() => Promise.resolve({ success: true })),
  recordFeedback: vi.fn(() => Promise.resolve({ success: true })),
  saveDraft: vi.fn(() => Promise.resolve({ success: true })),
  snoozeConversation: vi.fn(() => Promise.resolve({ success: true })),
  toggleReaction: vi.fn(() => Promise.resolve({ success: true })),
  unsnoozeConversation: vi.fn(() => Promise.resolve({ success: true })),
}))

vi.mock('@/store/auth', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/store/auth')>()
  return { ...actual, getToken: vi.fn(() => 'test-token') }
})

// React Query hooks: test runs without a real QueryClientProvider; stub
// useThreadQuery so the component can read state without setup overhead.
// Declared as a vi.fn so individual tests can override return via mockReturnValueOnce.
const mockUseThreadQuery = vi.fn<() => { data: ThreadMessage[] | undefined; isPending: boolean }>(
  () => ({ data: undefined, isPending: false })
)
vi.mock('@/hooks/use-mail-queries', () => ({
  useThreadQuery: mockUseThreadQuery,
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
  useDeleteMutation: stubMutation,
  useMarkReadMutation: stubMutation,
  useMarkUnreadMutation: stubMutation,
  useStarMutation: stubMutation,
  useUnstarMutation: stubMutation,
}))
vi.mock('@/lib/query-client', () => ({
  queryClient: { invalidateQueries: vi.fn(() => Promise.resolve()) },
}))
vi.mock('@/lib/query-keys', () => ({
  mailKeys: {
    conversations: () => ['mail', 'conversations'],
    // The Send and Draft sources are reached through
    // `useCurrentListRows`, which calls every one of them so the hook
    // order stays fixed as the list changes.
    drafts: () => ['mail', 'drafts'],
    sends: (status?: null | string) => ['mail', 'sends', status ?? ''],
    sent: () => ['mail', 'sent'],
    thread: (tid: null | string) => ['mail', 'thread', tid ?? ''],
  },
  // Reached by the invite card and by the dates offered from a body:
  // both read one message's detail, and share this key so opening a
  // message costs one request rather than two.
  messageKeys: {
    detail: (uid: number) => ['message', uid],
  },
}))

Element.prototype.scrollIntoView = vi.fn()

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))

vi.mock('@/components/ai-analysis', () => ({
  AiAnalysisPanel: ({ message }: { message: { summary?: string } }) => (
    <div data-testid="ai-analysis">{message.summary}</div>
  ),
}))

vi.mock('@/components/attachment-preview', () => ({
  AttachmentPreview: ({ attachments, uid }: { attachments: unknown[]; uid: number }) => (
    <div data-testid="attachment-preview">
      {attachments.length} attachment(s) for uid {uid}
    </div>
  ),
}))

vi.mock('@/components/message-bubble', () => ({
  MessageBubble: ({ htmlBody, textBody }: { htmlBody: null | string; textBody: null | string }) => (
    <div data-testid="message-bubble">{htmlBody ? 'HTML' : textBody ? 'TEXT' : 'EMPTY'}</div>
  ),
}))

vi.mock('@/components/category-badge', () => ({
  ActionBadge: () => <span data-testid="action-badge">Action</span>,
  CategoryBadge: ({ category }: { category: string }) =>
    category && category !== 'general' ? (
      <span data-testid="category-badge">{category}</span>
    ) : null,
  ImportanceBadge: ({ level }: { level: string }) =>
    level && level !== 'normal' ? <span data-testid="importance-badge">{level}</span> : null,
  IntentBadge: ({ intent }: { intent: string }) =>
    intent && intent !== 'inform' ? <span data-testid="intent-badge">{intent}</span> : null,
}))

vi.mock('@/components/reply-box', () => ({
  ReplyBox: ({ mode }: { mode: string }) => <div data-testid="reply-box">mode: {mode}</div>,
}))

function makeStore() {
  const store = createStore()
  store.set(authAtom, {
    accessible_domains: [],
    address: 'user@example.com',
    display_name: 'Test User',
    permissions: [],
    token: 'test-token',
  })
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

const { ThreadView } = await import('@/components/thread-view')

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('ThreadView — no selection', () => {
  it('shows empty state when no thread is selected', () => {
    // No rows, so the list has no current item — the selection is
    // derived now, so this is how "nothing is selected" is expressed.
    const store = makeStore()
    flatStub.conversations = []
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getByText('No conversation selected')).toBeDefined()
  })

  it('does not render back button in empty state (mobile nav handled by Chat)', () => {
    // No rows, so the list has no current item — the selection is
    // derived now, so this is how "nothing is selected" is expressed.
    const store = makeStore()
    flatStub.conversations = []
    const onBack = vi.fn()
    render(
      <Wrapper store={store}>
        <ThreadView onBack={onBack} />
      </Wrapper>
    )
    expect(screen.queryByText('Back')).toBeNull()
  })
})

describe('ThreadView — with messages', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    // The first row of the list IS the selection; nothing has to set it.
    flatStub.conversations = [makeConversation()]
  })

  it('renders thread subject in header', () => {
    mockUseThreadQuery.mockReturnValue({
      data: [makeMessage({ subject: 'Important Email' })],
      isPending: false,
    })
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    // subject now appears both in the header and inside the bubble
    expect(screen.getAllByText('Important Email').length).toBeGreaterThanOrEqual(1)
  })

  it('shows "(no subject)" when subject is empty', () => {
    mockUseThreadQuery.mockReturnValue({ data: [makeMessage({ subject: '' })], isPending: false })
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getAllByText('(no subject)').length).toBeGreaterThan(0)
  })

  it('displays message count', () => {
    mockUseThreadQuery.mockReturnValue({
      data: [makeMessage({ uid: 100 }), makeMessage({ sender: 'Bob <bob@example.com>', uid: 101 })],
      isPending: false,
    })
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    // header renders "<currentIdx>/<total>" once a message is auto-selected
    expect(screen.getByText(/\/?2$/)).toBeDefined()
  })

  it('hides count badge for single message', () => {
    mockUseThreadQuery.mockReturnValue({ data: [makeMessage()], isPending: false })
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.queryByText('1')).toBeNull()
  })

  it('renders sender name in chat bubble', () => {
    mockUseThreadQuery.mockReturnValue({
      data: [makeMessage({ sender: 'Charlie Brown <charlie@example.com>' })],
      isPending: false,
    })
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    // sender appears in both the bubble and the auto-selected message header
    expect(screen.getAllByText('Charlie Brown').length).toBeGreaterThan(0)
  })
})

describe('ThreadView — selected message detail', () => {
  async function renderAndWait(msg: ThreadMessage) {
    // Thread fetch lives in react-query (mocked at file top), so seed the
    // messages atom directly — the component reads from there for rendering.
    const store = makeStore()
    // The first row of the list IS the selection; nothing has to set it.
    flatStub.conversations = [makeConversation()]
    mockUseThreadQuery.mockReturnValue({ data: [msg], isPending: false })

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )

    await waitFor(() => {
      expect(screen.queryByText('Select a message to preview')).toBeNull()
    })
  }

  it('renders HTML content in raw email panel', async () => {
    await renderAndWait(makeMessage({ html_body: '<p>Hello</p>' }))
    expect(screen.getByTestId('message-bubble').textContent).toBe('HTML')
  })

  it('renders plain text in raw email panel', async () => {
    await renderAndWait(makeMessage({ html_body: null, text_body: 'Plain text email body here' }))
    // text appears in both raw panel and chat bubble snippet, use getAllByText
    expect(screen.getAllByText(/Plain text email body here/).length).toBeGreaterThanOrEqual(1)
  })

  /**
   * A mailing that arrived on 2026-08-14 with its body missing from both
   * MIME parts: a stylesheet, a `display:none` preheader and a tracking
   * gif. `html_body` was 2,785 bytes, so the HTML branch was chosen and
   * the reader got a white box. The text part is the only thing in the
   * message that says anything.
   */
  it('falls back to the text part when the html would paint nothing', async () => {
    await renderAndWait(
      makeMessage({
        html_body:
          '<html><head><style>.o_layout{}</style></head><body>' +
          '<div style="display:none">preheader only</div>' +
          '<img src="https://e.example/mail/track/blank.gif"/></body></html>',
        text_body: 'A small setup step can quietly become a major source of lost activation',
      })
    )
    expect(screen.queryByTestId('message-bubble')).toBeNull()
    expect(screen.getAllByText(/A small setup step/).length).toBeGreaterThanOrEqual(1)
  })

  it('renders attachment preview', async () => {
    await renderAndWait(
      makeMessage({
        attachments: [{ content_type: 'application/pdf', filename: 'doc.pdf', size: 1024 }],
      })
    )
    expect(screen.getByTestId('attachment-preview').textContent).toContain('1 attachment(s)')
  })

  it('shows risk badge for analyzed messages', async () => {
    await renderAndWait(makeMessage({ ai_analyzed: true, risk_score: 75 }))
    expect(screen.getByText(/Dangerous/)).toBeDefined()
  })

  it('shows Suspicious badge for medium risk', async () => {
    await renderAndWait(makeMessage({ ai_analyzed: true, risk_score: 50 }))
    expect(screen.getByText(/Suspicious/)).toBeDefined()
  })
})

describe('ThreadView — loading state', () => {
  it('shows skeleton when loading', async () => {
    mockUseThreadQuery.mockReturnValue({ data: undefined, isPending: true })

    const store = makeStore()
    // The first row of the list IS the selection; nothing has to set it.
    flatStub.conversations = [makeConversation()]

    const { container } = render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )

    await waitFor(() => {
      expect(container.querySelector('.animate-pulse')).not.toBeNull()
    })
  })

  it('does not show the empty state while a selected thread is loading', async () => {
    // regression: the loading spinner overlay (80% opacity) and the
    // "Select a message to preview" empty state used to render at the
    // same time — both visible through the translucent overlay.
    mockUseThreadQuery.mockReturnValue({ data: undefined, isPending: true })

    const store = makeStore()
    // The first row of the list IS the selection; nothing has to set it.
    flatStub.conversations = [makeConversation()]

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )

    await waitFor(() => {
      expect(screen.queryByText('Select a message to preview')).toBeNull()
    })
  })
})

describe('ThreadView — delete dialog', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    // The first row of the list IS the selection; nothing has to set it.
    flatStub.conversations = [makeConversation()]
    mockUseThreadQuery.mockReturnValue({ data: [makeMessage()], isPending: false })
  })

  it('shows delete dialog', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    fireEvent.click(screen.getByTitle('Delete'))
    expect(screen.getByText('Delete conversation?')).toBeDefined()
  })

  it('closes on cancel', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    fireEvent.click(screen.getByTitle('Delete'))
    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.queryByText('Delete conversation?')).toBeNull()
  })
})

describe('ThreadView — toolbar', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    // The first row of the list IS the selection; nothing has to set it.
    flatStub.conversations = [makeConversation()]
    mockUseThreadQuery.mockReturnValue({ data: [makeMessage()], isPending: false })
  })

  // The "Close" X is gone with the model that made it meaningful: a list
  // with rows always has a current one, so clearing the pick jumped to
  // the first row rather than emptying the pane.
  it('has no close button', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.queryByTitle('Close')).toBeNull()
  })

  it('renders reply box', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getByTestId('reply-box').textContent).toContain('mode: reply')
  })

  it('has star button', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getByTitle('Star')).toBeDefined()
  })

  it('has mark unread button', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getByTitle('Mark unread')).toBeDefined()
  })
})
