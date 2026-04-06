import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// jotai mocks: useAtomValue returns [] for array atoms, null/false for others
vi.mock('jotai', () => ({
  useAtomValue: vi.fn().mockImplementation((atom: symbol) => {
    const name = atom.description ?? ''
    if (name === 'domains') return []
    if (name === 'notifications' || name === 'sound') return false
    return null
  }),
  useSetAtom: vi.fn().mockReturnValue(vi.fn()),
}))
vi.mock('@/lib/api', () => ({ fetchJson: vi.fn().mockResolvedValue([]) }))
vi.mock('@/lib/notification-sound', () => ({ playNotificationSound: vi.fn() }))
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
  CONNECTING: number
  onclose: ((ev?: unknown) => void) | null
  onerror: ((ev?: unknown) => void) | null
  onmessage: ((ev: { data: string }) => void) | null
  onopen: (() => void) | null
  OPEN: number
  readyState: number
  send: ReturnType<typeof vi.fn>
} = null

let wsConstructCount = 0

class MockWebSocket {
  static CONNECTING = 0
  static OPEN = 1

  close = vi.fn()
  onclose: ((ev?: unknown) => void) | null = null
  onerror: ((ev?: unknown) => void) | null = null
  onmessage: ((ev: { data: string }) => void) | null = null
  onopen: (() => void) | null = null
  readyState = 1
  send = vi.fn()

  constructor() {
    mockWs = this as unknown as typeof mockWs
    wsConstructCount++
  }
}

describe('useMailEvents', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockWs = null
    wsConstructCount = 0
    vi.stubGlobal('WebSocket', MockWebSocket)
    vi.stubGlobal('localStorage', {
      getItem: vi.fn().mockReturnValue(JSON.stringify({ token: 'test-token' })),
    })
    vi.stubGlobal('location', { host: 'localhost:3200', protocol: 'http:' })
    // hook reads document.hidden and adds visibility listener
    vi.spyOn(document, 'addEventListener').mockImplementation(() => {})
    vi.spyOn(document, 'removeEventListener').mockImplementation(() => {})
    Object.defineProperty(document, 'hidden', {
      configurable: true,
      value: false,
    })
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

    // trigger onopen to start ping interval
    act(() => {
      ws.onopen?.()
    })

    // advance past one ping interval
    act(() => {
      vi.advanceTimersByTime(30_000)
    })

    expect(ws.send).toHaveBeenCalledWith('ping')
  })

  it('reconnects after close', async () => {
    await renderMailEvents('test@example.com')

    const firstCount = wsConstructCount

    // trigger close
    act(() => {
      mockWs?.onclose?.()
    })

    // reconnect delay is 3000ms
    act(() => {
      vi.advanceTimersByTime(3000)
    })

    expect(wsConstructCount).toBe(firstCount + 1)
  })
})
