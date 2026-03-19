import { Provider, createStore } from 'jotai'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'

import type { ConversationSummary, ThreadMessage } from '@/lib/types'

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

vi.mock('@/lib/api', () => ({
  fetchJson: vi.fn(() => Promise.resolve([])),
  postJson: vi.fn(() => Promise.resolve({ success: true })),
  deleteJson: vi.fn(() => Promise.resolve({ success: true })),
  saveDraft: vi.fn(() => Promise.resolve({ success: true })),
  getThreadReactions: vi.fn(() => Promise.resolve({})),
  toggleReaction: vi.fn(() => Promise.resolve({ success: true })),
  recordFeedback: vi.fn(() => Promise.resolve({ success: true })),
  snoozeConversation: vi.fn(() => Promise.resolve({ success: true })),
  unsnoozeConversation: vi.fn(() => Promise.resolve({ success: true })),
}))

vi.mock('@/store/auth', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/store/auth')>()
  return { ...actual, getToken: vi.fn(() => 'test-token') }
})

Element.prototype.scrollIntoView = vi.fn()

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock('@/components/ai-analysis', () => ({
  AiAnalysisPanel: ({ message }: { message: { summary?: string } }) => (
    <div data-testid="ai-analysis">{message.summary}</div>
  ),
}))

vi.mock('@/components/attachment-preview', () => ({
  AttachmentPreview: ({ attachments, uid }: { attachments: unknown[]; uid: number }) => (
    <div data-testid="attachment-preview">{attachments.length} attachment(s) for uid {uid}</div>
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
  ImportanceBadge: ({ level }: { level: string }) =>
    level && level !== 'normal' ? <span data-testid="importance-badge">{level}</span> : null,
  ActionBadge: () => <span data-testid="action-badge">Action</span>,
  IntentBadge: ({ intent }: { intent: string }) =>
    intent && intent !== 'inform' ? <span data-testid="intent-badge">{intent}</span> : null,
}))

vi.mock('@/components/reply-box', () => ({
  ReplyBox: ({ mode }: { mode: string }) => <div data-testid="reply-box">mode: {mode}</div>,
}))

function makeMessage(overrides: Partial<ThreadMessage> = {}): ThreadMessage {
  return {
    id: 1, uid: 100,
    sender: 'Alice Smith <alice@example.com>',
    recipients: 'bob@example.com',
    subject: 'Test Subject', flags: 0,
    internal_date: 1700000000,
    message_id: '<msg1@example.com>',
    text_body: 'Hello, this is a test message',
    html_body: null, attachments: [],
    category: 'general', risk_score: 0, risk_reason: '',
    summary: '', people: [], dates: [], amounts: [], action_items: [],
    ai_analyzed: false, clean_text: null,
    new_content: null, importance_level: 'normal', importance_score: 0.3,
    is_bulk_sender: false, has_tracking_pixel: false,
    requires_action: false, sender_intent: 'inform', action_deadline: null,
    ...overrides,
  }
}

function makeConversation(overrides: Partial<ConversationSummary> = {}): ConversationSummary {
  return {
    thread_id: 'thread-1', subject: 'Test Subject',
    participants: ['alice@example.com'], message_count: 1,
    unread_count: 0, last_date: Math.floor(Date.now() / 1000),
    category: 'general', flagged: false, snippet: 'A snippet',
    pinned: false, archived: false,
    importance_level: 'normal', importance_score: 0.3, requires_action: false, ...overrides,
  }
}

function makeStore() {
  const store = createStore()
  store.set(authAtom, {
    token: 'test-token', address: 'user@example.com',
    display_name: 'Test User', permissions: [], accessible_domains: [],
  })
  return store
}

function Wrapper({ store, children }: { store: ReturnType<typeof createStore>; children: ReactNode }) {
  return <Provider store={store}>{children}</Provider>
}

const { ThreadView } = await import('@/components/thread-view')

afterEach(() => { cleanup(); vi.clearAllMocks() })

describe('ThreadView — no selection', () => {
  it('shows empty state when no thread is selected', () => {
    const store = makeStore()
    store.set(selectedThreadIdAtom, null)
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getByText('No conversation selected')).toBeDefined()
  })

  it('does not render back button in empty state (mobile nav handled by Chat)', () => {
    const store = makeStore()
    store.set(selectedThreadIdAtom, null)
    const onBack = vi.fn()
    render(<Wrapper store={store}><ThreadView onBack={onBack} /></Wrapper>)
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
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getByText('Important Email')).toBeDefined()
  })

  it('shows "(no subject)" when subject is empty', () => {
    store.set(threadMessagesAtom, [makeMessage({ subject: '' })])
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getAllByText('(no subject)').length).toBeGreaterThan(0)
  })

  it('displays message count', () => {
    store.set(threadMessagesAtom, [
      makeMessage({ id: 1, uid: 100 }),
      makeMessage({ id: 2, uid: 101, sender: 'Bob <bob@example.com>' }),
    ])
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getByText('2')).toBeDefined()
  })

  it('hides count badge for single message', () => {
    store.set(threadMessagesAtom, [makeMessage()])
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.queryByText('1')).toBeNull()
  })

  it('renders sender name in chat bubble', () => {
    store.set(threadMessagesAtom, [makeMessage({ sender: 'Charlie Brown <charlie@example.com>' })])
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getByText('Charlie Brown')).toBeDefined()
  })
})

describe('ThreadView — selected message detail', () => {
  async function renderAndWait(msg: ThreadMessage) {
    const { fetchJson } = await import('@/lib/api')
    vi.mocked(fetchJson)
      .mockResolvedValueOnce([msg])           // loadMessages: thread messages

    const store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(selectedThreadIdAtom, 'thread-1')

    render(<Wrapper store={store}><ThreadView /></Wrapper>)

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
    await renderAndWait(makeMessage({ text_body: 'Plain text email body here', html_body: null }))
    // text appears in both raw panel and chat bubble snippet, use getAllByText
    expect(screen.getAllByText(/Plain text email body here/).length).toBeGreaterThanOrEqual(1)
  })

  it('renders attachment preview', async () => {
    await renderAndWait(makeMessage({
      attachments: [{ filename: 'doc.pdf', content_type: 'application/pdf', size: 1024 }],
    }))
    expect(screen.getByTestId('attachment-preview').textContent).toContain('1 attachment(s)')
  })

  it('shows risk badge for analyzed messages', async () => {
    await renderAndWait(makeMessage({ ai_analyzed: true, risk_score: 75 }))
    expect(screen.getByText(/Dangerous/)).toBeDefined()
  })

  it('shows Safe badge for low risk', async () => {
    await renderAndWait(makeMessage({ ai_analyzed: true, risk_score: 5 }))
    expect(screen.getByText(/Safe/)).toBeDefined()
  })
})

describe('ThreadView — loading state', () => {
  it('shows skeleton when loading', async () => {
    const { fetchJson } = await import('@/lib/api')
    vi.mocked(fetchJson).mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve([]), 500)),
    )

    const store = makeStore()
    store.set(conversationsAtom, [makeConversation()])
    store.set(threadMessagesAtom, [])
    store.set(selectedThreadIdAtom, 'thread-1')

    const { container } = render(<Wrapper store={store}><ThreadView /></Wrapper>)

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
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    fireEvent.click(screen.getByTitle('Delete'))
    expect(screen.getByText('Delete conversation?')).toBeDefined()
  })

  it('closes on cancel', () => {
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
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
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    fireEvent.click(screen.getByTitle('Close'))
    expect(store.get(selectedThreadIdAtom)).toBeNull()
  })

  it('renders reply box', () => {
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getByTestId('reply-box').textContent).toContain('mode: reply')
  })

  it('has star button', () => {
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getByTitle('Star')).toBeDefined()
  })

  it('has mark unread button', () => {
    render(<Wrapper store={store}><ThreadView /></Wrapper>)
    expect(screen.getByTitle('Mark unread')).toBeDefined()
  })
})

