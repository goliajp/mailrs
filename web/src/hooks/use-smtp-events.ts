import { useCallback, useEffect, useRef, useState } from 'react'

import type {
  ConnectionInfo,
  ConversationLine,
  ServerStatus,
  SmtpEvent,
} from '@/lib/types'

function getWsUrl() {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
  const token = localStorage.getItem('mailrs_auth')
  const parsed = token ? JSON.parse(token) : null
  const tokenParam = parsed?.token ? `?token=${encodeURIComponent(parsed.token)}` : ''
  return `${proto}//${location.host}/api/events${tokenParam}`
}
const STATUS_URL = '/api/status'

export function useSmtpEvents() {
  const [connections, setConnections] = useState<Map<number, ConnectionInfo>>(
    new Map()
  )
  const [events, setEvents] = useState<SmtpEvent[]>([])
  const [status, setStatus] = useState<ServerStatus | null>(null)
  const [connected, setConnected] = useState(false)
  const wsRef = useRef<WebSocket | null>(null)

  const handleEvent = useCallback((event: SmtpEvent) => {
    setEvents((prev) => [...prev.slice(-500), event])

    setConnections((prev) => {
      const next = new Map(prev)

      switch (event.type) {
        case 'ConnectionOpened':
          next.set(event.id, {
            id: event.id,
            addr: event.addr,
            tls: event.tls,
            state: 'Connected',
            lines: [],
          })
          break

        case 'CommandReceived': {
          const conn = next.get(event.id)
          if (conn) {
            const line: ConversationLine = {
              direction: 'client',
              text: event.command,
              timestamp: Date.now(),
            }
            next.set(event.id, {
              ...conn,
              state: event.state_before,
              lines: [...conn.lines, line],
            })
          }
          break
        }

        case 'ResponseSent': {
          const conn = next.get(event.id)
          if (conn) {
            const line: ConversationLine = {
              direction: 'server',
              text: event.response,
              timestamp: Date.now(),
            }
            next.set(event.id, {
              ...conn,
              state: event.state_after,
              lines: [...conn.lines, line],
            })
          }
          break
        }

        case 'TlsUpgraded': {
          const conn = next.get(event.id)
          if (conn) {
            next.set(event.id, { ...conn, tls: true })
          }
          break
        }

        case 'Authenticated': {
          const conn = next.get(event.id)
          if (conn) {
            next.set(event.id, { ...conn, authenticated: event.username })
          }
          break
        }

        case 'ConnectionClosed':
          next.delete(event.id)
          break
      }

      return next
    })
  }, [])

  useEffect(() => {
    let ws: WebSocket
    let retryTimeout: ReturnType<typeof setTimeout>

    const connect = () => {
      ws = new WebSocket(getWsUrl())
      wsRef.current = ws

      ws.onopen = () => setConnected(true)
      ws.onclose = () => {
        setConnected(false)
        retryTimeout = setTimeout(connect, 2000)
      }
      ws.onerror = () => ws.close()
      ws.onmessage = (msg) => {
        try {
          const event = JSON.parse(msg.data) as SmtpEvent
          handleEvent(event)
        } catch {
          // ignore malformed messages
        }
      }
    }

    connect()
    return () => {
      clearTimeout(retryTimeout)
      ws?.close()
    }
  }, [handleEvent])

  useEffect(() => {
    const poll = () => {
      fetch(STATUS_URL)
        .then((r) => r.json())
        .then((data) => setStatus(data as ServerStatus))
        .catch(() => {})
    }
    poll()
    const interval = setInterval(poll, 3000)
    return () => clearInterval(interval)
  }, [])

  return { connections, events, status, connected }
}
