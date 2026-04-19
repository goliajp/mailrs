import type { ConversationSummary, ThreadMessage } from '@/lib/types'
import type { ReactNode } from 'react'

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
import { conversationsAtom, selectedThreadIdAtom, threadMessagesAtom } from '@/store/chat'

vi.mock('@/lib/api', () => ({
  deleteJson: vi.fn(() => Promise.resolve({ success: true })),
  fetchJson: vi.fn(() => Promise.resolve([])),
  getThreadReactions: vi.fn(() => Promise.resolve({})),
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
    requires_action: false,
    snippet: 'A snippet',
    subject: 'Test Subject',
    thread_id: 'thread-1',
    unread_count: 0,
    ...overrides,
  }
}

function makeMessage(overrides: Partial<ThreadMessage> = {}): ThreadMessage {
  return {
    action_deadline: null,
    action_items: [],
    ai_analyzed: false,
    amounts: [],
    attachments: [],
    category: 'general',
    clean_text: null,
    dates: [],
    flags: 0,
    has_tracking_pixel: false,
    html_body: null,
    id: 1,
    importance_level: 'normal',
    importance_score: 0.3,
    internal_date: 1700000000,
    is_bulk_sender: false,
    message_id: '<msg1@example.com>',
    new_content: null,
    people: [],
    recipients: 'bob@example.com',
    requires_action: false,
    risk_reason: '',
    risk_score: 0,
    sender: 'Alice Smith <alice@example.com>',
    sender_intent: 'inform',
    subject: 'Test Subject',
    summary: '',
    text_body: 'Hello, this is a test message',
    uid: 100,
    ...overrides,
  }
}

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

function Wrapper({
  children,
  store,
}: {
  children: ReactNode
  store: ReturnType<typeof createStore>
}) {
  return <Provider store={store}>{children}</Provider>
}

const { ThreadView } = await import('@/components/thread-view')

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('ThreadView — no selection', () => {
  it('shows empty state when no thread is selected', () => {
    const store = makeStore()
    store.set(selectedThreadIdAtom, null)
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getByText('No conversation selected')).toBeDefined()
  })

  it('does not render back button in empty state (mobile nav handled by Chat)', () => {
    const store = makeStore()
    store.set(selectedThreadIdAtom, null)
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
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')
  })

  it('renders thread subject in header', () => {
    store.set(threadMessagesAtom, [makeMessage({ subject: 'Important Email' })])
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    // subject now appears both in the header and inside the bubble
    expect(screen.getAllByText('Important Email').length).toBeGreaterThanOrEqual(1)
  })

  it('shows "(no subject)" when subject is empty', () => {
    store.set(threadMessagesAtom, [makeMessage({ subject: '' })])
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getAllByText('(no subject)').length).toBeGreaterThan(0)
  })

  it('displays message count', () => {
    store.set(threadMessagesAtom, [
      makeMessage({ id: 1, uid: 100 }),
      makeMessage({ id: 2, sender: 'Bob <bob@example.com>', uid: 101 }),
    ])
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getByText('2')).toBeDefined()
  })

  it('hides count badge for single message', () => {
    store.set(threadMessagesAtom, [makeMessage()])
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.queryByText('1')).toBeNull()
  })

  it('renders sender name in chat bubble', () => {
    store.set(threadMessagesAtom, [makeMessage({ sender: 'Charlie Brown <charlie@example.com>' })])
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    expect(screen.getByText('Charlie Brown')).toBeDefined()
  })
})

describe('ThreadView — selected message detail', () => {
  async function renderAndWait(msg: ThreadMessage) {
    const { fetchJson } = await import('@/lib/api')
    vi.mocked(fetchJson).mockResolvedValueOnce([msg]) // loadMessages: thread messages

    const store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')

    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )

    // wait for loadMessages to resolve and selectedMsgIdx to be set
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
    const { fetchJson } = await import('@/lib/api')
    vi.mocked(fetchJson).mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve([]), 500))
    )

    const store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(threadMessagesAtom, [])
    store.set(selectedThreadIdAtom, 'thread-1')

    const { container } = render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )

    await waitFor(() => {
      expect(container.querySelector('.animate-pulse')).not.toBeNull()
    })
  })
})

describe('ThreadView — delete dialog', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')
    store.set(threadMessagesAtom, [makeMessage()])
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
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')
    store.set(threadMessagesAtom, [makeMessage()])
  })

  it('close button clears selection', () => {
    render(
      <Wrapper store={store}>
        <ThreadView />
      </Wrapper>
    )
    fireEvent.click(screen.getByTitle('Close'))
    expect(store.get(selectedThreadIdAtom)).toBeNull()
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
