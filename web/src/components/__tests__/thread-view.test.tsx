import { Provider, createStore } from 'jotai'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'

import type { ConversationSummary, ThreadMessage } from '@/lib/types'

// stub localStorage before importing auth atom
const localStorageStore: Record<string, string> = {}
vi.stubGlobal('localStorage', {
  getItem: vi.fn((key: string) => localStorageStore[key] ?? null),
  setItem: vi.fn((key: string, value: string) => { localStorageStore[key] = value }),
  removeItem: vi.fn((key: string) => { delete localStorageStore[key] }),
  clear: vi.fn(() => { Object.keys(localStorageStore).forEach((k) => delete localStorageStore[k]) }),
  length: 0,
  key: vi.fn(() => null),
})

import { authAtom } from '@/store/auth'
import {
  conversationsAtom,
  selectedThreadIdAtom,
  threadMessagesAtom,
} from '@/store/chat'

// mock api module
vi.mock('@/lib/api', () => ({
  fetchJson: vi.fn(() => Promise.resolve([])),
  postJson: vi.fn(() => Promise.resolve({ success: true })),
  deleteJson: vi.fn(() => Promise.resolve({ success: true })),
}))

// mock store/auth getToken
vi.mock('@/store/auth', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/store/auth')>()
  return {
    ...actual,
    getToken: vi.fn(() => 'test-token'),
  }
})

// stub scrollIntoView for jsdom
Element.prototype.scrollIntoView = vi.fn()

// mock sonner toast
vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

// mock child components to isolate ThreadView logic
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
  MessageBubble: ({ htmlBody, textBody }: { htmlBody: string | null; textBody: string | null }) => (
    <div data-testid="message-bubble">{htmlBody ? 'HTML' : textBody ? 'TEXT' : 'EMPTY'}</div>
  ),
}))

vi.mock('@/components/category-badge', () => ({
  CategoryBadge: ({ category }: { category: string }) =>
    category && category !== 'general' ? <span data-testid="category-badge">{category}</span> : null,
}))

vi.mock('@/components/reply-box', () => ({
  ReplyBox: ({ mode }: { mode: string }) => <div data-testid="reply-box">mode: {mode}</div>,
}))

function makeMessage(overrides: Partial<ThreadMessage> = {}): ThreadMessage {
  return {
    id: 1,
    uid: 100,
    sender: 'Alice Smith <alice@example.com>',
    recipients: 'bob@example.com',
    subject: 'Test Subject',
    flags: 0,
    internal_date: 1700000000,
    message_id: '<msg1@example.com>',
    text_body: 'Hello, this is a test message',
    html_body: null,
    attachments: [],
    category: 'general',
    risk_score: 0,
    risk_reason: '',
    summary: '',
    people: [],
    dates: [],
    amounts: [],
    action_items: [],
    ai_analyzed: false,
    clean_text: null,
    ...overrides,
  }
}

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
    ...overrides,
  }
}

function makeStore() {
  const store = createStore()
  store.set(authAtom, {
    token: 'test-token',
    address: 'user@example.com',
    display_name: 'Test User',
    super_domains: [],
  })
  return store
}

function Wrapper({ store, children }: { store: ReturnType<typeof createStore>; children: ReactNode }) {
  return <Provider store={store}>{children}</Provider>
}

// must import after mocks
const { ThreadView } = await import('@/components/thread-view')

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('ThreadView — no selection', () => {
  it('shows "Select a conversation" when no thread is selected', () => {
    const store = makeStore()
    store.set(selectedThreadIdAtom, null)

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('Select a conversation')).toBeDefined()
  })

  it('renders mobile back button when onBack is provided and no selection', () => {
    const store = makeStore()
    store.set(selectedThreadIdAtom, null)
    const onBack = vi.fn()

    render(
      <Wrapper store={store}>
        <ThreadView onBack={onBack} />
      </Wrapper>,
    )

    const backButton = screen.getByText('Back')
    fireEvent.click(backButton)
    expect(onBack).toHaveBeenCalledTimes(1)
  })
})

describe('ThreadView — with selection and messages', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')
  })

  it('renders thread subject from messages', () => {
    store.set(threadMessagesAtom, [makeMessage({ subject: 'Important Email' })])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('Important Email')).toBeDefined()
  })

  it('shows "(no subject)" when subject is empty', () => {
    store.set(threadMessagesAtom, [makeMessage({ subject: '' })])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    // the header shows (no subject)
    const noSubjects = screen.getAllByText('(no subject)')
    expect(noSubjects.length).toBeGreaterThan(0)
  })

  it('displays message count', () => {
    store.set(threadMessagesAtom, [
      makeMessage({ id: 1, uid: 100 }),
      makeMessage({ id: 2, uid: 101, sender: 'Bob <bob@example.com>' }),
    ])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('2 messages')).toBeDefined()
  })

  it('shows singular "message" for single message', () => {
    store.set(threadMessagesAtom, [makeMessage()])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('1 message')).toBeDefined()
  })

  it('renders sender name in message list', () => {
    store.set(threadMessagesAtom, [makeMessage({ sender: 'Charlie Brown <charlie@example.com>' })])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('Charlie Brown')).toBeDefined()
  })

  it('shows "You" for messages from the authenticated user', () => {
    store.set(threadMessagesAtom, [makeMessage({ sender: 'Test User <user@example.com>' })])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('You')).toBeDefined()
  })

  it('renders message preview text', () => {
    store.set(threadMessagesAtom, [makeMessage({ clean_text: 'Preview of the message body' })])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('Preview of the message body')).toBeDefined()
  })

  it('falls back to text_body for preview when clean_text is null', () => {
    store.set(threadMessagesAtom, [
      makeMessage({ clean_text: null, text_body: 'Fallback text body preview' }),
    ])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('Fallback text body preview')).toBeDefined()
  })

  it('shows "(no content)" when no text content available', () => {
    store.set(threadMessagesAtom, [
      makeMessage({ clean_text: null, text_body: null, subject: '' }),
    ])

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByText('(no content)')).toBeDefined()
  })
})

describe('ThreadView — selected message detail', () => {
  let store: ReturnType<typeof createStore>

  // helper: render thread view, wait for loadMessages to populate, then click the message
  async function renderAndOpenDetail(msg: ThreadMessage) {
    const { fetchJson } = await import('@/lib/api')
    const mockFetch = vi.mocked(fetchJson)
    // loadMessages calls fetchJson twice: first for thread messages, then for conversation list refresh
    mockFetch
      .mockResolvedValueOnce([msg])
      .mockResolvedValueOnce([makeConversation()])

    store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    // wait for loadMessages to resolve and render message cards, then click to open detail
    await waitFor(() => {
      const buttons = screen.getAllByRole('button')
      const msgButton = buttons.find((b) => b.className.includes('flex w-full gap-3'))
      expect(msgButton).toBeDefined()
      if (msgButton) fireEvent.click(msgButton)
    })
  }

  it('renders HTML content via MessageBubble when html_body is present', async () => {
    await renderAndOpenDetail(makeMessage({ html_body: '<p>Hello HTML</p>' }))
    expect(screen.getByTestId('message-bubble').textContent).toBe('HTML')
  })

  it('renders plain text section', async () => {
    await renderAndOpenDetail(makeMessage({ text_body: 'Plain text email body', html_body: null }))
    expect(screen.getByText('Plain Text')).toBeDefined()
  })

  it('shows "AI Extracted Text" label when clean_text is present', async () => {
    await renderAndOpenDetail(makeMessage({ clean_text: 'AI extracted content', text_body: null }))
    expect(screen.getByText('AI Extracted Text')).toBeDefined()
  })

  it('shows "(no text content)" when both text fields are null', async () => {
    await renderAndOpenDetail(makeMessage({ clean_text: null, text_body: null }))
    expect(screen.getAllByText('(no text content)').length).toBeGreaterThanOrEqual(1)
  })

  it('renders attachment preview component', async () => {
    await renderAndOpenDetail(makeMessage({
      attachments: [
        { filename: 'doc.pdf', content_type: 'application/pdf', size: 1024 },
      ],
    }))
    const preview = screen.getByTestId('attachment-preview')
    expect(preview.textContent).toContain('1 attachment(s)')
  })

  it('renders risk badge for high risk messages', async () => {
    await renderAndOpenDetail(makeMessage({ ai_analyzed: true, risk_score: 75 }))
    expect(screen.getByText(/Dangerous/)).toBeDefined()
  })

  it('renders "Safe" badge for low risk analyzed messages', async () => {
    await renderAndOpenDetail(makeMessage({ ai_analyzed: true, risk_score: 5 }))
    expect(screen.getByText(/Safe/)).toBeDefined()
  })

  it('renders "Suspicious" badge for medium risk messages', async () => {
    await renderAndOpenDetail(makeMessage({ ai_analyzed: true, risk_score: 45 }))
    expect(screen.getByText(/Suspicious/)).toBeDefined()
  })

  it('renders "Caution" badge for moderate risk messages', async () => {
    await renderAndOpenDetail(makeMessage({ ai_analyzed: true, risk_score: 20 }))
    expect(screen.getByText(/Caution/)).toBeDefined()
  })

  it('does not render risk badge for non-analyzed messages', async () => {
    await renderAndOpenDetail(makeMessage({ ai_analyzed: false, risk_score: 80 }))
    expect(screen.queryByText(/Dangerous/)).toBeNull()
  })
})

describe('ThreadView — loading state', () => {
  it('shows skeleton placeholders when loading with no messages', async () => {
    const { fetchJson } = await import('@/lib/api')
    const mockFetchJson = vi.mocked(fetchJson)
    // delay the response to keep loading state visible
    mockFetchJson.mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve([]), 500)),
    )

    const store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(threadMessagesAtom, [])
    store.set(selectedThreadIdAtom, 'thread-1')

    const { container } = render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    // loading skeleton uses animate-pulse class
    await waitFor(() => {
      const pulse = container.querySelector('.animate-pulse')
      expect(pulse).not.toBeNull()
    })
  })
})

describe('ThreadView — delete confirmation dialog', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')
    store.set(threadMessagesAtom, [makeMessage()])
  })

  it('shows delete confirmation dialog when delete button is clicked', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    const deleteButton = screen.getByTitle('Delete conversation')
    fireEvent.click(deleteButton)

    expect(screen.getByText('Delete conversation?')).toBeDefined()
    expect(screen.getByText(/permanently delete/)).toBeDefined()
  })

  it('closes delete confirmation dialog on cancel', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    fireEvent.click(screen.getByTitle('Delete conversation'))
    expect(screen.getByText('Delete conversation?')).toBeDefined()

    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.queryByText('Delete conversation?')).toBeNull()
  })
})

describe('ThreadView — toolbar actions', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')
    store.set(threadMessagesAtom, [makeMessage()])
  })

  it('has a close button that clears the selected thread', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    const closeButton = screen.getByLabelText('Close conversation')
    fireEvent.click(closeButton)

    // after closing, selectedThreadIdAtom should be null
    expect(store.get(selectedThreadIdAtom)).toBeNull()
  })

  it('renders reply box with default reply mode', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    const replyBox = screen.getByTestId('reply-box')
    expect(replyBox.textContent).toContain('mode: reply')
  })

  it('renders star/unstar button', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByTitle('Star')).toBeDefined()
  })

  it('renders mark unread button when message is read', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>,
    )

    expect(screen.getByTitle('Mark as unread')).toBeDefined()
  })
})
