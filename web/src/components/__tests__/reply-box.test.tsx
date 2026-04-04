import type { ReactNode } from 'react'

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ReplyBox, type ReplyMode } from '@/components/reply-box'
import { authAtom } from '@/store/auth'

// mock api module to prevent real network calls
vi.mock('@/lib/api', () => ({
  fetchJson: vi.fn(() => Promise.resolve([])),
  postJson: vi.fn(() => Promise.resolve({ success: true })),
  saveDraft: vi.fn(() => Promise.resolve({ success: true })),
}))

// mock sonner toast
vi.mock('sonner', () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
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

const defaultProps = {
  lastMessageId: 'msg-1',
  mode: 'reply' as ReplyMode,
  onModeChange: vi.fn(),
  onSent: vi.fn(),
  originalBody: 'Original message body',
  originalDate: '2024-01-01 10:00',
  originalFrom: 'alice@example.com',
  replyAllRecipients: 'alice@example.com, bob@example.com',
  replyRecipients: 'alice@example.com',
  subject: 'Test Subject',
  threadId: 'thread-1',
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('ReplyBox', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
  })

  it('renders all three mode buttons', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} />
      </Wrapper>
    )

    expect(screen.getByText('Reply')).toBeDefined()
    expect(screen.getByText('Reply All')).toBeDefined()
    expect(screen.getByText('Forward')).toBeDefined()
  })

  it('highlights the active mode button', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply" />
      </Wrapper>
    )

    const replyButton = screen.getByText('Reply')
    expect(replyButton.className).toContain('bg-accent/10')
  })

  it('calls onModeChange when switching to reply-all', () => {
    const onModeChange = vi.fn()
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} onModeChange={onModeChange} />
      </Wrapper>
    )

    fireEvent.click(screen.getByText('Reply All'))
    expect(onModeChange).toHaveBeenCalledWith('reply-all')
  })

  it('calls onModeChange when switching to forward', () => {
    const onModeChange = vi.fn()
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} onModeChange={onModeChange} />
      </Wrapper>
    )

    fireEvent.click(screen.getByText('Forward'))
    expect(onModeChange).toHaveBeenCalledWith('forward')
  })

  it('shows recipient preview in reply mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply" />
      </Wrapper>
    )

    expect(screen.getByText('to alice@example.com')).toBeDefined()
  })

  it('shows all recipients in reply-all mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply-all" />
      </Wrapper>
    )

    expect(screen.getByText('to alice@example.com, bob@example.com')).toBeDefined()
  })

  it('shows forward To field in forward mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="forward" />
      </Wrapper>
    )

    // forward mode should render the ContactAutocomplete input
    const input = screen.getByPlaceholderText('recipient@example.com')
    expect(input).toBeDefined()
  })

  it('hides recipient preview in forward mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="forward" />
      </Wrapper>
    )

    expect(screen.queryByText(/^to alice/)).toBeNull()
  })

  it('renders send button', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} />
      </Wrapper>
    )

    const sendButton = screen.getByRole('button', { name: /send/i })
    expect(sendButton).toBeDefined()
  })

  it('renders add block button', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} />
      </Wrapper>
    )

    expect(screen.getByText('Add block')).toBeDefined()
  })

  it('highlights forward mode button when forward is active', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="forward" />
      </Wrapper>
    )

    const forwardButton = screen.getByText('Forward')
    expect(forwardButton.className).toContain('bg-accent/10')
  })

  it('does not highlight inactive mode buttons', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply" />
      </Wrapper>
    )

    const replyAllButton = screen.getByText('Reply All')
    expect(replyAllButton.className).not.toContain('bg-blue-100')

    const forwardButton = screen.getByText('Forward')
    expect(forwardButton.className).not.toContain('bg-blue-100')
  })
})
