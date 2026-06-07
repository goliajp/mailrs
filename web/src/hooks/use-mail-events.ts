import type { ConversationSummary, NewMessageEvent, SmtpEvent } from '@/lib/types'
import type { InfiniteData } from '@tanstack/react-query'

import { useAtomValue, useSetAtom } from 'jotai'
import { useEffect, useRef } from 'react'

import { playNotificationSound } from '@/lib/notification-sound'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { getNotificationSupport, safeStorage } from '@/lib/safe-storage'
import { connectionStatusAtom, selectedThreadIdAtom } from '@/store/chat'
import { notificationsAtom, notificationSoundAtom } from '@/store/settings'

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
      const token = safeStorage.getItem('mailrs_auth')
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
          const event = JSON.parse(e.data) as NewMessageEvent | SmtpEvent
          if (event.type === 'NewMessage') {
            const msg = event as NewMessageEvent
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
              patchConversationCaches(msg)
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
                if (getNotificationSupport() === 'granted' && document.hidden) {
                  try {
                    new Notification(msg.sender, {
                      body: msg.subject || msg.snippet,
                      tag: msg.thread_id,
                    })
                  } catch {
                    // some sandboxed contexts allow permission but throw on construction
                  }
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

// Surgical conversation-cache update for a `NewMessage` WebSocket event.
//
// For every cached `mailKeys.conversations(*)` entry, look for the thread:
//   - found → patch the entry in place (bump counters, refresh snippet /
//     last_sender / last_date) and move it to the top of page 0 with no
//     network round-trip.
//   - not found → invalidate only that cache, exactly. Other caches stay
//     untouched. The active filter refetches; idle filters wait until next
//     mount.
//
// Replaces the previous `invalidateQueries({ queryKey: mailKeys.conversations() })`
// which marked every cached filter stale and refetched all loaded pages
// of the active filter on every received mail. For a multi-page inbox
// that's 5+ HTTP round-trips per inbound message; here it's 0 for the
// common reply-in-existing-thread case.
function patchConversationCaches(event: NewMessageEvent): void {
  const entries = queryClient.getQueriesData<InfiniteData<ConversationSummary[]>>({
    queryKey: mailKeys.conversations(),
  })
  for (const [key, data] of entries) {
    if (!data) continue
    let foundPage = -1
    let foundIdx = -1
    for (let p = 0; p < data.pages.length; p++) {
      const idx = data.pages[p].findIndex((c) => c.thread_id === event.thread_id)
      if (idx >= 0) {
        foundPage = p
        foundIdx = idx
        break
      }
    }
    if (foundPage >= 0) {
      const sourcePage = foundPage
      const sourceIdx = foundIdx
      queryClient.setQueryData<InfiniteData<ConversationSummary[]>>(key, (old) => {
        if (!old) return old
        const existing = old.pages[sourcePage]?.[sourceIdx]
        if (!existing) return old
        const updated: ConversationSummary = {
          ...existing,
          last_date: Math.floor(Date.now() / 1000),
          last_sender: event.sender,
          message_count: existing.message_count + 1,
          received_count: existing.received_count + 1,
          snippet: event.snippet,
          subject: event.subject || existing.subject,
          unread_count: existing.unread_count + 1,
        }
        const newPages = old.pages.map((page, p) =>
          p === sourcePage ? page.filter((_, i) => i !== sourceIdx) : page
        )
        newPages[0] = [updated, ...newPages[0]]
        return { ...old, pages: newPages }
      })
    } else {
      queryClient.invalidateQueries({ exact: true, queryKey: key }).catch(() => {})
    }
  }
}
