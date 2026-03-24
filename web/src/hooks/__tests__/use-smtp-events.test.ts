import type { SmtpEvent } from '@/lib/types'

import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// mock WebSocket
let mockWs: {
  close: ReturnType<typeof vi.fn>
  onclose: ((ev?: unknown) => void) | null
  onerror: ((ev?: unknown) => void) | null
  onmessage: ((ev: { data: string }) => void) | null
  onopen: (() => void) | null
  readyState: number
  send: ReturnType<typeof vi.fn>
}

class MockWebSocket {
  close = vi.fn()
  onclose: ((ev?: unknown) => void) | null = null
  onerror: ((ev?: unknown) => void) | null = null
  onmessage: ((ev: { data: string }) => void) | null = null
  onopen: (() => void) | null = null
  readyState = 1
  send = vi.fn()

  constructor() {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    mockWs = this
  }
}

let mockFetch: ReturnType<typeof vi.fn>

describe('useSmtpEvents', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockFetch = vi.fn().mockResolvedValue({
      json: () =>
        Promise.resolve({
          active_connections: 0,
          total_connections: 0,
          total_messages: 0,
          uptime_secs: 100,
        }),
    })
    vi.stubGlobal('WebSocket', MockWebSocket)
    vi.stubGlobal('fetch', mockFetch)
    vi.stubGlobal('localStorage', {
      getItem: vi.fn().mockReturnValue(JSON.stringify({ token: 'test-token' })),
    })
    vi.stubGlobal('location', { host: 'localhost:3200', protocol: 'http:' })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  // must import lazily so globals are stubbed before module evaluates
  async function renderSmtpEvents() {
    const { useSmtpEvents } = await import('../use-smtp-events')
    return renderHook(() => useSmtpEvents())
  }

  function sendEvent(event: SmtpEvent) {
    act(() => {
      mockWs.onmessage?.({ data: JSON.stringify(event) })
    })
  }

  it('returns initial state', async () => {
    const { result } = await renderSmtpEvents()

    expect(result.current.connected).toBe(false)
    expect(result.current.connections.size).toBe(0)
    expect(result.current.events).toEqual([])
    expect(result.current.status).toBeNull()
  })

  it('sets connected=true on ws open', async () => {
    const { result } = await renderSmtpEvents()

    act(() => {
      mockWs.onopen?.()
    })

    expect(result.current.connected).toBe(true)
  })

  it('handles ConnectionOpened event', async () => {
    const { result } = await renderSmtpEvents()

    act(() => {
      mockWs.onopen?.()
    })

    sendEvent({
      addr: '127.0.0.1:12345',
      id: 1,
      tls: false,
      type: 'ConnectionOpened',
    })

    expect(result.current.connections.size).toBe(1)
    const conn = result.current.connections.get(1)
    expect(conn).toEqual({
      addr: '127.0.0.1:12345',
      id: 1,
      lines: [],
      state: 'Connected',
      tls: false,
    })
    expect(result.current.events).toHaveLength(1)
  })

  it('handles ConnectionClosed event', async () => {
    const { result } = await renderSmtpEvents()

    act(() => {
      mockWs.onopen?.()
    })
    sendEvent({
      addr: '127.0.0.1:12345',
      id: 1,
      tls: false,
      type: 'ConnectionOpened',
    })
    expect(result.current.connections.size).toBe(1)

    sendEvent({ id: 1, type: 'ConnectionClosed' })
    expect(result.current.connections.size).toBe(0)
  })

  it('handles Authenticated event', async () => {
    const { result } = await renderSmtpEvents()

    act(() => {
      mockWs.onopen?.()
    })
    sendEvent({
      addr: '127.0.0.1:12345',
      id: 1,
      tls: false,
      type: 'ConnectionOpened',
    })
    sendEvent({ id: 1, type: 'Authenticated', username: 'alice@example.com' })

    const conn = result.current.connections.get(1)
    expect(conn?.authenticated).toBe('alice@example.com')
  })

  it('handles CommandReceived event', async () => {
    const { result } = await renderSmtpEvents()

    act(() => {
      mockWs.onopen?.()
    })
    sendEvent({
      addr: '127.0.0.1:12345',
      id: 1,
      tls: false,
      type: 'ConnectionOpened',
    })
    sendEvent({
      command: 'EHLO example.com',
      id: 1,
      state_before: 'Connected',
      type: 'CommandReceived',
    })

    const conn = result.current.connections.get(1)
    expect(conn?.lines).toHaveLength(1)
    expect(conn?.lines[0].direction).toBe('client')
    expect(conn?.lines[0].text).toBe('EHLO example.com')
    expect(conn?.state).toBe('Connected')
  })

  it('handles ResponseSent event', async () => {
    const { result } = await renderSmtpEvents()

    act(() => {
      mockWs.onopen?.()
    })
    sendEvent({
      addr: '127.0.0.1:12345',
      id: 1,
      tls: false,
      type: 'ConnectionOpened',
    })
    sendEvent({
      id: 1,
      response: '250 OK',
      state_after: 'MailFrom',
      type: 'ResponseSent',
    })

    const conn = result.current.connections.get(1)
    expect(conn?.lines).toHaveLength(1)
    expect(conn?.lines[0].direction).toBe('server')
    expect(conn?.lines[0].text).toBe('250 OK')
    expect(conn?.state).toBe('MailFrom')
  })

  it('handles TlsUpgraded event', async () => {
    const { result } = await renderSmtpEvents()

    act(() => {
      mockWs.onopen?.()
    })
    sendEvent({
      addr: '127.0.0.1:12345',
      id: 1,
      tls: false,
      type: 'ConnectionOpened',
    })
    expect(result.current.connections.get(1)?.tls).toBe(false)

    sendEvent({ id: 1, type: 'TlsUpgraded' })
    expect(result.current.connections.get(1)?.tls).toBe(true)
  })

  it('reconnects on close', async () => {
    await renderSmtpEvents()

    const firstWs = mockWs

    act(() => {
      firstWs.onclose?.()
    })

    // retry timeout is 2000ms
    act(() => {
      vi.advanceTimersByTime(2000)
    })

    // a new WebSocket should have been constructed
    expect(mockWs).not.toBe(firstWs)
  })

  it('polls status on mount', async () => {
    await renderSmtpEvents()

    // fetch is called once immediately for status
    expect(mockFetch).toHaveBeenCalledWith('/api/status')
  })
})
