import { Provider, createStore } from 'jotai'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'

import { authAtom } from '@/store/auth'
import { ReplyBox, type ReplyMode } from '@/components/reply-box'

// mock api module to prevent real network calls
vi.mock('@/lib/api', () => ({
  fetchJson: vi.fn(() => Promise.resolve([])),
  postJson: vi.fn(() => Promise.resolve({ success: true })),
  saveDraft: vi.fn(() => Promise.resolve({ success: true })),
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

function makeStore() {
  const store = createStore()
  store.set(authAtom, {
    token: 'test-token',
    address: 'user@example.com',
    display_name: 'Test User',
    permissions: [],
    accessible_domains: [],
  })
  return store
}

function Wrapper({ store, children }: { store: ReturnType<typeof createStore>; children: ReactNode }) {
  return <Provider store={store}>{children}</Provider>
}

const defaultProps = {
  threadId: 'thread-1',
  lastMessageId: 'msg-1',
  replyRecipients: 'alice@example.com',
  replyAllRecipients: 'alice@example.com, bob@example.com',
  subject: 'Test Subject',
  originalFrom: 'alice@example.com',
  originalDate: '2024-01-01 10:00',
  originalBody: 'Original message body',
  onSent: vi.fn(),
  mode: 'reply' as ReplyMode,
  onModeChange: vi.fn(),
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
      </Wrapper>,
    )

    expect(screen.getByText('Reply')).toBeDefined()
    expect(screen.getByText('Reply All')).toBeDefined()
    expect(screen.getByText('Forward')).toBeDefined()
  })

  it('highlights the active mode button', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply" />
      </Wrapper>,
    )

    const replyButton = screen.getByText('Reply')
    expect(replyButton.className).toContain('bg-[var(--color-brand-subtle)]')
  })

  it('calls onModeChange when switching to reply-all', () => {
    const onModeChange = vi.fn()
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} onModeChange={onModeChange} />
      </Wrapper>,
    )

    fireEvent.click(screen.getByText('Reply All'))
    expect(onModeChange).toHaveBeenCalledWith('reply-all')
  })

  it('calls onModeChange when switching to forward', () => {
    const onModeChange = vi.fn()
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} onModeChange={onModeChange} />
      </Wrapper>,
    )

    fireEvent.click(screen.getByText('Forward'))
    expect(onModeChange).toHaveBeenCalledWith('forward')
  })

  it('shows recipient preview in reply mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply" />
      </Wrapper>,
    )

    expect(screen.getByText('to alice@example.com')).toBeDefined()
  })

  it('shows all recipients in reply-all mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply-all" />
      </Wrapper>,
    )

    expect(screen.getByText('to alice@example.com, bob@example.com')).toBeDefined()
  })

  it('shows forward To field in forward mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="forward" />
      </Wrapper>,
    )

    // forward mode should render the ContactAutocomplete input
    const input = screen.getByPlaceholderText('To: recipient@example.com, ...')
    expect(input).toBeDefined()
  })

  it('hides recipient preview in forward mode', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="forward" />
      </Wrapper>,
    )

    expect(screen.queryByText(/^to alice/)).toBeNull()
  })

  it('renders send button', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} />
      </Wrapper>,
    )

    const sendButton = screen.getByTitle('Send (Ctrl+Enter)')
    expect(sendButton).toBeDefined()
  })

  it('renders draft button', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} />
      </Wrapper>,
    )

    const draftButton = screen.getByTitle('Save draft')
    expect(draftButton).toBeDefined()
  })

  it('renders attach file button', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} />
      </Wrapper>,
    )

    expect(screen.getByTitle('Attach file')).toBeDefined()
  })

  it('renders hidden file input for attachments', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} />
      </Wrapper>,
    )

    const fileInput = screen.getByLabelText('Attach files')
    expect(fileInput).toBeDefined()
    expect((fileInput as HTMLInputElement).type).toBe('file')
  })

  it('highlights forward mode button when forward is active', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="forward" />
      </Wrapper>,
    )

    const forwardButton = screen.getByText('Forward')
    expect(forwardButton.className).toContain('bg-[var(--color-brand-subtle)]')
  })

  it('does not highlight inactive mode buttons', () => {
    render(
      <Wrapper store={store}>
        <ReplyBox {...defaultProps} mode="reply" />
      </Wrapper>,
    )

    const replyAllButton = screen.getByText('Reply All')
    expect(replyAllButton.className).not.toContain('bg-blue-100')

    const forwardButton = screen.getByText('Forward')
    expect(forwardButton.className).not.toContain('bg-blue-100')
  })
})
