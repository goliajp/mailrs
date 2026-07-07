import type { ConversationSummary, NewMessageEvent, SmtpEvent } from '@/lib/types'

import { useAtomValue, useSetAtom } from 'jotai'
import { useEffect, useRef } from 'react'

import { playNotificationSound } from '@/lib/notification-sound'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { onNewMessage } from '@/reducers/events/conversation'
import { notificationsAtom, notificationSoundAtom } from '@/store/settings'
import { connectionStatusAtom, selectedThreadIdAtom } from '@/store/ui'

// shallow equality over the conversation fields ConversationItem actually
// renders. Used to preserve object identity across refetches so memo'd
// rows don't re-render when their payload is unchanged. Exported for tests
// and for chat.tsx's bridge from query data to legacy atoms.
export function shallowEqualConvo(a: ConversationSummary, b: ConversationSummary): boolean {
  if (a === b) return true
  if (
    a.thread_id !== b.thread_id ||
    a.subject !== b.subject ||
    a.snippet !== b.snippet ||
    a.last_date !== b.last_date ||
    a.unread_count !== b.unread_count ||
    a.message_count !== b.message_count ||
    a.flagged !== b.flagged ||
    a.pinned !== b.pinned ||
    a.archived !== b.archived
  ) {
    return false
  }
  if (a.participants.length !== b.participants.length) return false
  for (let i = 0; i < a.participants.length; i++) {
    if (a.participants[i] !== b.participants[i]) return false
  }
  return true
}

const POLL_INTERVAL = 60_000
const WS_PING_INTERVAL = 30_000
const RECONNECT_BASE = 1_000
const RECONNECT_MAX = 30_000

export function useMailEvents(user: string) {
  const setConnectionStatus = useSetAtom(connectionStatusAtom)
  const selectedThreadId = useAtomValue(selectedThreadIdAtom)
  const selectedRef = useRef(selectedThreadId)
  selectedRef.current = selectedThreadId
  const notificationsEnabled = useAtomValue(notificationsAtom)
  const notificationsRef = useRef(notificationsEnabled)
  notificationsRef.current = notificationsEnabled
  const soundEnabled = useAtomValue(notificationSoundAtom)
  const soundRef = useRef(soundEnabled)
  soundRef.current = soundEnabled

  const wsRef = useRef<null | WebSocket>(null)
  const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(null)
  const pingTimer = useRef<ReturnType<typeof setInterval>>(null)
  const pollTimer = useRef<ReturnType<typeof setInterval>>(null)

  useEffect(() => {
    if (!user) return
    let closed = false
    let reconnectDelay = RECONNECT_BASE

    // Invalidate cached mail queries so any subscribed component triggers
    // a quiet refetch. RQ deduplicates concurrent calls, so a burst of
    // events still collapses to one network round-trip per query.
    const invalidateAllMail = () => {
      queryClient.invalidateQueries({ queryKey: mailKeys.conversations() }).catch(() => {})
      const tid = selectedRef.current
      if (tid) {
        queryClient.invalidateQueries({ queryKey: mailKeys.thread(tid) }).catch(() => {})
      }
    }

    function connect() {
      if (closed) return
      if (!navigator.onLine) return
      if (pingTimer.current) clearInterval(pingTimer.current)

      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
      const token = localStorage.getItem('mailrs_auth')
      const parsed = token ? JSON.parse(token) : null
      const tokenParam = parsed?.token ? `?token=${encodeURIComponent(parsed.token)}` : ''
      const ws = new WebSocket(`${proto}//${location.host}/api/events${tokenParam}`)
      wsRef.current = ws

      ws.onopen = () => {
        reconnectDelay = RECONNECT_BASE
        setConnectionStatus('connected')
        pingTimer.current = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send('ping')
          }
        }, WS_PING_INTERVAL)
      }

      ws.onmessage = (e) => {
        try {
          const parsed: unknown = JSON.parse(e.data)
          if (
            !parsed ||
            typeof parsed !== 'object' ||
            typeof (parsed as { type?: unknown }).type !== 'string'
          ) {
            return
          }
          const event = parsed as NewMessageEvent | SmtpEvent
          if (event.type === 'NewMessage') {
            const msg = event
            if (msg.user === user) {
              // Surgical cache update instead of a blanket
              // `invalidateQueries({ queryKey: mailKeys.conversations() })`
              // — that variant marks every cached filter stale and
              // refetches all loaded pages of the active filter on every
              // received mail. For existing threads (the common case
              // when replies arrive in an ongoing conversation) we can
              // patch the cached entry in place and move it to the top
              // of page 0 with zero network. For genuinely new threads
              // we still need the server's filter logic — but invalidate
              // only the cache that doesn't already know the thread,
              // not every conversations-shaped cache the user ever
              // visited.
              // v2.1 phase-4: cache mutation is now a pure reducer
              // (`reducers/events/conversation.ts::onNewMessage`).
              // The hook layer just dispatches; every "how do we
              // patch RQ on this event" decision is testable in
              // isolation. Local `patchConversationCaches` kept until
              // the next Phase pass cleans it up.
              onNewMessage(queryClient, msg)
              queryClient.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
              queryClient.invalidateQueries({ queryKey: mailKeys.actionCount([]) }).catch(() => {})
              // also bust the thread the event belongs to, if it's the one
              // we're viewing
              if (selectedRef.current && msg.thread_id === selectedRef.current) {
                queryClient
                  .invalidateQueries({ queryKey: mailKeys.thread(selectedRef.current) })
                  .catch(() => {})
              }

              if (notificationsRef.current) {
                if (soundRef.current) playNotificationSound()
                // 'Notification' is absent on iOS Safari (non-PWA)
                if (
                  'Notification' in window &&
                  Notification.permission === 'granted' &&
                  document.hidden
                ) {
                  new Notification(msg.sender, {
                    body: msg.subject || msg.snippet,
                    tag: msg.thread_id,
                  })
                }
              }
            }
          }
        } catch {
          // ignore
        }
      }

      ws.onclose = () => {
        if (pingTimer.current) clearInterval(pingTimer.current)
        setConnectionStatus(navigator.onLine ? 'connecting' : 'offline')
        if (!closed) {
          reconnectTimer.current = setTimeout(connect, reconnectDelay)
          reconnectDelay = Math.min(reconnectDelay * 2, RECONNECT_MAX)
        }
      }

      ws.onerror = () => {
        ws.close()
      }
    }

    connect()

    // periodic polling as fallback — only when WS is not connected and tab is visible
    function startPolling() {
      if (pollTimer.current) clearInterval(pollTimer.current)
      pollTimer.current = setInterval(() => {
        if (document.hidden) return
        if (wsRef.current?.readyState === WebSocket.OPEN) return
        invalidateAllMail()
      }, POLL_INTERVAL)
    }
    startPolling()

    function onVisibilityChange() {
      if (document.visibilityState === 'visible') {
        invalidateAllMail()
        if (
          wsRef.current &&
          wsRef.current.readyState !== WebSocket.OPEN &&
          wsRef.current.readyState !== WebSocket.CONNECTING
        ) {
          connect()
        }
      }
    }
    document.addEventListener('visibilitychange', onVisibilityChange)

    function onOnline() {
      reconnectDelay = RECONNECT_BASE
      if (
        wsRef.current &&
        wsRef.current.readyState !== WebSocket.OPEN &&
        wsRef.current.readyState !== WebSocket.CONNECTING
      ) {
        connect()
      }
      invalidateAllMail()
    }

    function onOffline() {
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current)
      setConnectionStatus('offline')
    }

    window.addEventListener('online', onOnline)
    window.addEventListener('offline', onOffline)

    return () => {
      closed = true
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current)
      if (pingTimer.current) clearInterval(pingTimer.current)
      if (pollTimer.current) clearInterval(pollTimer.current)
      document.removeEventListener('visibilitychange', onVisibilityChange)
      window.removeEventListener('online', onOnline)
      window.removeEventListener('offline', onOffline)
      wsRef.current?.close()
      wsRef.current = null
    }
  }, [user, setConnectionStatus])
}

// v2.1 phase-4: the previous local `patchConversationCaches` moved to
// `reducers/events/conversation.ts::onNewMessage`. This hook now
// dispatches to that reducer — see the onmessage handler above.
