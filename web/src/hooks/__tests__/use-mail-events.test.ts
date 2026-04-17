import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mockSetConversations = vi.fn()
const mockSetConnectionStatus = vi.fn()
const mockSetThreadMessages = vi.fn()

// jotai mocks: useAtomValue returns context-appropriate defaults; useSetAtom returns
// stable spies so tests can assert what the hook calls
vi.mock('jotai', () => ({
  useAtomValue: vi.fn().mockImplementation((atom: symbol) => {
    const name = atom.description ?? ''
    if (name === 'domains') return []
    if (name === 'notifications' || name === 'sound') return true
    if (name === 'quickFilter') return 'all'
    return null
  }),
  useSetAtom: vi.fn().mockImplementation((atom: symbol) => {
    const name = atom.description ?? ''
    if (name === 'conversations') return mockSetConversations
    if (name === 'connectionStatus') return mockSetConnectionStatus
    if (name === 'messages') return mockSetThreadMessages
    return vi.fn()
  }),
}))
const mockFetchJson = vi.fn().mockResolvedValue([])
vi.mock('@/lib/api', () => ({ fetchJson: mockFetchJson }))
const mockPlaySound = vi.fn()
vi.mock('@/lib/notification-sound', () => ({ playNotificationSound: mockPlaySound }))
vi.mock('@/store/chat', () => ({
  categoryFilterAtom: Symbol('category'),
  connectionStatusAtom: Symbol('connectionStatus'),
  conversationsAtom: Symbol('conversations'),
  folderAtom: Symbol('folder'),
  importanceSectionAtom: Symbol('section'),
  quickFilterAtom: Symbol('quickFilter'),
  searchQueryAtom: Symbol('search'),
  selectedDomainsAtom: Symbol('domains'),
  selectedThreadIdAtom: Symbol('selected'),
  threadMessagesAtom: Symbol('messages'),
}))
vi.mock('@/store/settings', () => ({
  notificationsAtom: Symbol('notifications'),
  notificationSoundAtom: Symbol('sound'),
}))

// mock WebSocket
let mockWs: null | {
  close: ReturnType<typeof vi.fn>
  CLOSED: number
  CLOSING: number
  CONNECTING: number
  onclose: ((ev?: unknown) => void) | null
  onerror: ((ev?: unknown) => void) | null
  onmessage: ((ev: { data: string }) => void) | null
  onopen: (() => void) | null
  OPEN: number
  readyState: number
  send: ReturnType<typeof vi.fn>
  url: string
} = null

let wsConstructCount = 0
const wsUrls: string[] = []

type DocListenerMap = Record<string, ((ev?: unknown) => void)[]>

type WindowListenerMap = Record<string, ((ev?: unknown) => void)[]>
class MockWebSocket {
  static CLOSED = 3
  static CLOSING = 2
  static CONNECTING = 0
  static OPEN = 1

  close = vi.fn()
  onclose: ((ev?: unknown) => void) | null = null
  onerror: ((ev?: unknown) => void) | null = null
  onmessage: ((ev: { data: string }) => void) | null = null
  onopen: (() => void) | null = null
  readyState = 1
  send = vi.fn()
  url: string

  constructor(url: string) {
    this.url = url
    wsUrls.push(url)
    mockWs = this as unknown as typeof mockWs
    wsConstructCount++
  }
}

let docListeners: DocListenerMap
let windowListeners: WindowListenerMap

describe('useMailEvents', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockWs = null
    wsConstructCount = 0
    wsUrls.length = 0
    docListeners = {}
    windowListeners = {}
    mockSetConversations.mockClear()
    mockSetConnectionStatus.mockClear()
    mockSetThreadMessages.mockClear()
    mockFetchJson.mockClear()
    mockFetchJson.mockResolvedValue([])
    mockPlaySound.mockClear()

    vi.stubGlobal('WebSocket', MockWebSocket)
    vi.stubGlobal('localStorage', {
      getItem: vi.fn().mockReturnValue(JSON.stringify({ token: 'test-token' })),
    })
    vi.stubGlobal('location', { host: 'localhost:3200', protocol: 'http:' })
    vi.stubGlobal('navigator', { onLine: true })

    // capture document/window listeners so tests can fire them
    vi.spyOn(document, 'addEventListener').mockImplementation((type, fn) => {
      ;(docListeners[type as string] ||= []).push(fn as (ev?: unknown) => void)
    })
    vi.spyOn(document, 'removeEventListener').mockImplementation(() => {})
    vi.spyOn(window, 'addEventListener').mockImplementation((type, fn) => {
      ;(windowListeners[type as string] ||= []).push(fn as (ev?: unknown) => void)
    })
    vi.spyOn(window, 'removeEventListener').mockImplementation(() => {})

    Object.defineProperty(document, 'hidden', { configurable: true, value: false })
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  async function renderMailEvents(user: string) {
    const { useMailEvents } = await import('../use-mail-events')
    return renderHook(() => useMailEvents(user))
  }

  function fireWindow(evt: string) {
    for (const fn of windowListeners[evt] ?? []) act(() => fn())
  }

  function fireDoc(evt: string) {
    for (const fn of docListeners[evt] ?? []) act(() => fn())
  }

  it('creates WebSocket connection when user is provided', async () => {
    await renderMailEvents('test@example.com')

    expect(mockWs).not.toBeNull()
    expect(wsConstructCount).toBe(1)
  })

  it('does not connect when user is empty', async () => {
    await renderMailEvents('')

    expect(mockWs).toBeNull()
    expect(wsConstructCount).toBe(0)
  })

  it('cleans up on unmount', async () => {
    const { unmount } = await renderMailEvents('test@example.com')

    const ws = mockWs!
    unmount()

    expect(ws.close).toHaveBeenCalled()
  })

  it('sends periodic pings', async () => {
    await renderMailEvents('test@example.com')

    const ws = mockWs!
    ws.readyState = MockWebSocket.OPEN

    act(() => {
      ws.onopen?.()
    })

    act(() => {
      vi.advanceTimersByTime(30_000)
    })

    expect(ws.send).toHaveBeenCalledWith('ping')
  })

  it('skips ping when socket is not open', async () => {
    await renderMailEvents('test@example.com')

    const ws = mockWs!
    act(() => {
      ws.onopen?.()
    })
    ws.readyState = MockWebSocket.CLOSED

    act(() => {
      vi.advanceTimersByTime(30_000)
    })

    expect(ws.send).not.toHaveBeenCalled()
  })

  it('reconnects after close', async () => {
    await renderMailEvents('test@example.com')

    const firstCount = wsConstructCount
    act(() => {
      mockWs?.onclose?.()
    })

    act(() => {
      vi.advanceTimersByTime(3000)
    })

    expect(wsConstructCount).toBe(firstCount + 1)
  })

  it('reports connection status on open', async () => {
    await renderMailEvents('test@example.com')

    act(() => {
      mockWs?.onopen?.()
    })

    expect(mockSetConnectionStatus).toHaveBeenCalledWith('connected')
  })

  it('reports connecting status on close while online', async () => {
    await renderMailEvents('test@example.com')

    act(() => {
      mockWs?.onclose?.()
    })

    expect(mockSetConnectionStatus).toHaveBeenCalledWith('connecting')
  })

  it('skips initial connect when offline', async () => {
    vi.stubGlobal('navigator', { onLine: false })

    await renderMailEvents('test@example.com')

    expect(mockWs).toBeNull()
  })

  it('closes socket on error', async () => {
    await renderMailEvents('test@example.com')
    const ws = mockWs!

    act(() => {
      ws.onerror?.()
    })

    expect(ws.close).toHaveBeenCalled()
  })

  it('refreshes conversations on NewMessage for current user', async () => {
    await renderMailEvents('alice@example.com')
    const ws = mockWs!

    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({
          sender: 'bob@example.com',
          snippet: 'hello',
          subject: 'Hi',
          thread_id: 'tid-1',
          type: 'NewMessage',
          user: 'alice@example.com',
        }),
      })
    })

    expect(mockFetchJson).toHaveBeenCalled()
  })

  it('ignores NewMessage destined for different user', async () => {
    await renderMailEvents('alice@example.com')
    const ws = mockWs!
    mockFetchJson.mockClear()

    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({
          sender: 'bob@example.com',
          snippet: 'hello',
          subject: 'Hi',
          thread_id: 'tid-1',
          type: 'NewMessage',
          user: 'someone-else@example.com',
        }),
      })
    })

    expect(mockFetchJson).not.toHaveBeenCalled()
  })

  it('plays sound when notifications and sound enabled', async () => {
    await renderMailEvents('alice@example.com')
    const ws = mockWs!

    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({
          sender: 'bob@example.com',
          snippet: 'hello',
          subject: 'Hi',
          thread_id: 'tid-1',
          type: 'NewMessage',
          user: 'alice@example.com',
        }),
      })
    })

    expect(mockPlaySound).toHaveBeenCalled()
  })

  it('swallows malformed JSON in onmessage', async () => {
    await renderMailEvents('alice@example.com')
    const ws = mockWs!

    expect(() =>
      act(() => {
        ws.onmessage?.({ data: 'not-json' })
      })
    ).not.toThrow()
  })

  it('uses wss protocol when on https', async () => {
    vi.stubGlobal('location', { host: 'mail.example.com', protocol: 'https:' })

    await renderMailEvents('alice@example.com')

    expect(wsUrls[0]).toMatch(/^wss:/)
  })

  it('uses ws protocol when on http', async () => {
    vi.stubGlobal('location', { host: 'localhost:3200', protocol: 'http:' })

    await renderMailEvents('alice@example.com')

    expect(wsUrls[0]).toMatch(/^ws:/)
  })

  it('omits token param when localStorage has no token', async () => {
    vi.stubGlobal('localStorage', { getItem: vi.fn().mockReturnValue(null) })

    await renderMailEvents('alice@example.com')

    expect(wsUrls[0]).not.toContain('?token=')
  })

  it('refreshes data on visibility change to visible', async () => {
    await renderMailEvents('alice@example.com')
    mockFetchJson.mockClear()

    fireDoc('visibilitychange')

    expect(mockFetchJson).toHaveBeenCalled()
  })

  it('skips refresh on visibility change when hidden', async () => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' })
    await renderMailEvents('alice@example.com')
    mockFetchJson.mockClear()

    fireDoc('visibilitychange')

    expect(mockFetchJson).not.toHaveBeenCalled()
  })

  it('reconnects on online event when socket is dead', async () => {
    await renderMailEvents('alice@example.com')
    const before = wsConstructCount
    mockWs!.readyState = MockWebSocket.CLOSED

    fireWindow('online')

    expect(wsConstructCount).toBeGreaterThan(before)
  })

  it('marks connection offline on offline event', async () => {
    await renderMailEvents('alice@example.com')
    mockSetConnectionStatus.mockClear()

    fireWindow('offline')

    expect(mockSetConnectionStatus).toHaveBeenCalledWith('offline')
  })

  it('shows desktop notification when granted and tab hidden', async () => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: true })

    const NotificationMock = vi.fn()
    ;(NotificationMock as unknown as { permission: string }).permission = 'granted'
    vi.stubGlobal('Notification', NotificationMock)

    await renderMailEvents('alice@example.com')

    act(() => {
      mockWs!.onmessage?.({
        data: JSON.stringify({
          sender: 'bob@example.com',
          snippet: 'hello',
          subject: 'Hi',
          thread_id: 'tid-1',
          type: 'NewMessage',
          user: 'alice@example.com',
        }),
      })
    })

    expect(NotificationMock).toHaveBeenCalledWith('bob@example.com', expect.any(Object))
  })

  it('skips desktop notification when permission denied', async () => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: true })

    const NotificationMock = vi.fn()
    ;(NotificationMock as unknown as { permission: string }).permission = 'denied'
    vi.stubGlobal('Notification', NotificationMock)

    await renderMailEvents('alice@example.com')

    act(() => {
      mockWs!.onmessage?.({
        data: JSON.stringify({
          sender: 'bob@example.com',
          snippet: 'hi',
          subject: 'sub',
          thread_id: 't',
          type: 'NewMessage',
          user: 'alice@example.com',
        }),
      })
    })

    expect(NotificationMock).not.toHaveBeenCalled()
  })

  it('polls fallback refresh when ws is closed and tab visible', async () => {
    await renderMailEvents('alice@example.com')
    mockWs!.readyState = MockWebSocket.CLOSED
    mockFetchJson.mockClear()

    act(() => {
      vi.advanceTimersByTime(60_000)
    })

    expect(mockFetchJson).toHaveBeenCalled()
  })

  it('skips poll fallback when ws is open', async () => {
    await renderMailEvents('alice@example.com')
    act(() => {
      mockWs!.onopen?.()
    })
    mockWs!.readyState = MockWebSocket.OPEN
    mockFetchJson.mockClear()

    act(() => {
      vi.advanceTimersByTime(60_000)
    })

    expect(mockFetchJson).not.toHaveBeenCalled()
  })

  it('skips poll fallback when document hidden', async () => {
    await renderMailEvents('alice@example.com')
    Object.defineProperty(document, 'hidden', { configurable: true, value: true })
    mockWs!.readyState = MockWebSocket.CLOSED
    mockFetchJson.mockClear()

    act(() => {
      vi.advanceTimersByTime(60_000)
    })

    expect(mockFetchJson).not.toHaveBeenCalled()
  })

  it('reconnects on visibility change when socket is dead', async () => {
    await renderMailEvents('alice@example.com')
    const before = wsConstructCount
    mockWs!.readyState = MockWebSocket.CLOSED

    fireDoc('visibilitychange')

    expect(wsConstructCount).toBeGreaterThan(before)
  })

  it('does not reconnect on visibility change when socket is open', async () => {
    await renderMailEvents('alice@example.com')
    act(() => {
      mockWs!.onopen?.()
    })
    const before = wsConstructCount
    mockWs!.readyState = MockWebSocket.OPEN

    fireDoc('visibilitychange')

    expect(wsConstructCount).toBe(before)
  })
})
