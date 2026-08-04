import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mockSetConversations = vi.fn()
const mockSetConnectionStatus = vi.fn()
const mockSetThreadMessages = vi.fn()

// jotai mocks: useAtomValue returns context-appropriate defaults; useSetAtom returns
// stable spies so tests can assert what the hook calls
// The selection is derived from the current list's rows, and this file
// mocks jotai wholesale — so stub the reader rather than drag the whole
// query stack in behind it.
vi.mock('@/hooks/use-current-list', () => ({
  useSelectedThreadId: () => null,
}))

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
const mockInvalidateQueries = vi.fn().mockResolvedValue(undefined)
const mockGetQueriesData = vi.fn().mockReturnValue([])
const mockSetQueryData = vi.fn()
vi.mock('@/lib/query-client', () => ({
  queryClient: {
    getQueriesData: mockGetQueriesData,
    invalidateQueries: mockInvalidateQueries,
    setQueryData: mockSetQueryData,
  },
}))
vi.mock('@/lib/query-keys', () => ({
  mailKeys: {
    all: () => ['mail'],
    categories: () => ['mail', 'categories'],
    conversations: () => ['mail', 'conversations'],
    thread: (tid: string) => ['mail', 'thread', tid],
  },
}))
const mockPlaySound = vi.fn()
vi.mock('@/lib/notification-sound', () => ({ playNotificationSound: mockPlaySound }))
vi.mock('@/store/ui', () => ({
  categoryFilterAtom: Symbol('category'),
  connectionStatusAtom: Symbol('connectionStatus'),
  folderAtom: Symbol('folder'),
  importanceSectionAtom: Symbol('section'),
  quickFilterAtom: Symbol('quickFilter'),
  searchQueryAtom: Symbol('search'),
  selectedDomainsAtom: Symbol('domains'),
  selectedThreadIdAtom: Symbol('selected'),
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

describe('useMailEvents — socket', () => {
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
    const { useMailEvents } = await import('../../use-mail-events')
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

  describe('shallowEqualConvo', () => {
    type Convo = {
      archived: boolean
      category: string
      flagged: boolean
      importance_level: string
      importance_score: number
      last_date: number
      message_count: number
      participants: string[]
      pinned: boolean
      received_count: number
      requires_action: boolean
      sent_count: number
      snippet: string
      subject: string
      thread_id: string
      unread_count: number
    }
    const baseline: Convo = {
      archived: false,
      category: 'inbox',
      flagged: false,
      importance_level: 'normal',
      importance_score: 0,
      last_date: 100,
      message_count: 2,
      participants: ['a@x.com', 'b@x.com'],
      pinned: false,
      received_count: 1,
      requires_action: false,
      sent_count: 1,
      snippet: 's',
      subject: 't',
      thread_id: 'id-1',
      unread_count: 0,
    }

    it('returns true for same reference', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      expect(shallowEqualConvo(baseline, baseline)).toBe(true)
    })

    it('returns true for shallowly equal objects with different references', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      const clone = { ...baseline, participants: [...baseline.participants] }
      expect(shallowEqualConvo(baseline, clone)).toBe(true)
    })

    it('detects subject / snippet changes', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      expect(shallowEqualConvo(baseline, { ...baseline, subject: 'new' })).toBe(false)
      expect(shallowEqualConvo(baseline, { ...baseline, snippet: 'new' })).toBe(false)
    })

    it('detects flagged / pinned / archived flips', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      expect(shallowEqualConvo(baseline, { ...baseline, flagged: true })).toBe(false)
      expect(shallowEqualConvo(baseline, { ...baseline, pinned: true })).toBe(false)
      expect(shallowEqualConvo(baseline, { ...baseline, archived: true })).toBe(false)
    })

    it('detects unread / message count and last_date changes', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      expect(shallowEqualConvo(baseline, { ...baseline, unread_count: 1 })).toBe(false)
      expect(shallowEqualConvo(baseline, { ...baseline, message_count: 3 })).toBe(false)
      expect(shallowEqualConvo(baseline, { ...baseline, last_date: 200 })).toBe(false)
    })

    it('detects participants change by length', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      const more = { ...baseline, participants: [...baseline.participants, 'c@x.com'] }
      expect(shallowEqualConvo(baseline, more)).toBe(false)
    })

    it('detects participants change at same length', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      const swapped = { ...baseline, participants: ['z@x.com', baseline.participants[1]] }
      expect(shallowEqualConvo(baseline, swapped)).toBe(false)
    })

    it('detects thread_id change', async () => {
      const { shallowEqualConvo } = await import('../../use-mail-events')
      expect(shallowEqualConvo(baseline, { ...baseline, thread_id: 'id-2' })).toBe(false)
    })
  })
})
