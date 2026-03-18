import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef } from 'react'

import { fetchJson } from '@/lib/api'
import { playNotificationSound } from '@/lib/notification-sound'
import type { ConversationSummary, NewMessageEvent, SmtpEvent, ThreadMessage } from '@/lib/types'
import { categoryFilterAtom, conversationsAtom, folderAtom, searchQueryAtom, selectedDomainsAtom, selectedThreadIdAtom, threadMessagesAtom } from '@/store/chat'
import { notificationsAtom, notificationSoundAtom } from '@/store/settings'

const POLL_INTERVAL = 15_000
const WS_PING_INTERVAL = 30_000

export function useMailEvents(user: string) {
  const setConversations = useSetAtom(conversationsAtom)
  const setThreadMessages = useSetAtom(threadMessagesAtom)
  const selectedThreadId = useAtomValue(selectedThreadIdAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedRef = useRef(selectedThreadId)
  selectedRef.current = selectedThreadId
  const categoryRef = useRef(categoryFilter)
  categoryRef.current = categoryFilter
  const searchQuery = useAtomValue(searchQueryAtom)
  const searchRef = useRef(searchQuery)
  searchRef.current = searchQuery
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const domainsRef = useRef(selectedDomains)
  domainsRef.current = selectedDomains
  const folder = useAtomValue(folderAtom)
  const folderRef = useRef(folder)
  folderRef.current = folder
  const notificationsEnabled = useAtomValue(notificationsAtom)
  const notificationsRef = useRef(notificationsEnabled)
  notificationsRef.current = notificationsEnabled
  const soundEnabled = useAtomValue(notificationSoundAtom)
  const soundRef = useRef(soundEnabled)
  soundRef.current = soundEnabled

  const wsRef = useRef<WebSocket | null>(null)
  const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(null)
  const pingTimer = useRef<ReturnType<typeof setInterval>>(null)
  const pollTimer = useRef<ReturnType<typeof setInterval>>(null)

  const refreshConversations = useCallback(() => {
    const sq = searchRef.current
    let path = sq
      ? `/conversations/search?q=${encodeURIComponent(sq)}&limit=50`
      : '/conversations?limit=50'
    if (categoryRef.current) {
      path += `&category=${encodeURIComponent(categoryRef.current)}`
    }
    const doms = domainsRef.current
    if (doms.length > 0) {
      path += `&domains=${encodeURIComponent(doms.join(','))}`
    }
    const f = folderRef.current
    if (f) {
      path += `&folder=${encodeURIComponent(f)}`
    }
    fetchJson<ConversationSummary[]>(path).then(
      (data) => setConversations(data),
      () => {}
    )
  }, [setConversations])

  const refreshThread = useCallback(() => {
    const tid = selectedRef.current
    if (!tid) return
    const doms = domainsRef.current
    const domainsParam = doms.length > 0 ? `?domains=${encodeURIComponent(doms.join(','))}` : ''
    fetchJson<ThreadMessage[]>(
      `/conversations/${encodeURIComponent(tid)}${domainsParam}`
    ).then(
      (data) => setThreadMessages(data),
      () => {}
    )
  }, [setThreadMessages])

  const refreshAll = useCallback(() => {
    refreshConversations()
    refreshThread()
  }, [refreshConversations, refreshThread])

  useEffect(() => {
    if (!user) return
    let closed = false

    function connect() {
      if (closed) return
      // clear previous ping timer
      if (pingTimer.current) clearInterval(pingTimer.current)

      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
      const token = localStorage.getItem('mailrs_auth')
      const parsed = token ? JSON.parse(token) : null
      const tokenParam = parsed?.token ? `?token=${encodeURIComponent(parsed.token)}` : ''
      const ws = new WebSocket(`${proto}//${location.host}/api/events${tokenParam}`)
      wsRef.current = ws

      ws.onopen = () => {
        // send periodic pings to keep connection alive and detect dead sockets
        pingTimer.current = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send('ping')
          }
        }, WS_PING_INTERVAL)
      }

      ws.onmessage = (e) => {
        try {
          const event = JSON.parse(e.data) as SmtpEvent | NewMessageEvent
          if (event.type === 'NewMessage') {
            const msg = event as NewMessageEvent
            if (msg.user === user) {
              refreshConversations()

              // refresh current thread if the new message belongs to it
              if (selectedRef.current && msg.thread_id === selectedRef.current) {
                refreshThread()
              }

              if (notificationsRef.current) {
                if (soundRef.current) playNotificationSound()

                if (typeof Notification !== 'undefined' && Notification.permission === 'granted' && document.hidden) {
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
        if (!closed) {
          reconnectTimer.current = setTimeout(connect, 3000)
        }
      }

      ws.onerror = () => {
        ws.close()
      }
    }

    connect()

    // periodic polling as fallback for when WS is silently dead
    pollTimer.current = setInterval(refreshAll, POLL_INTERVAL)

    // refresh on tab visibility change
    function onVisibilityChange() {
      if (document.visibilityState === 'visible') {
        refreshAll()
        // also reconnect WS if it's dead
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

    return () => {
      closed = true
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current)
      if (pingTimer.current) clearInterval(pingTimer.current)
      if (pollTimer.current) clearInterval(pollTimer.current)
      document.removeEventListener('visibilitychange', onVisibilityChange)
      wsRef.current?.close()
      wsRef.current = null
    }
  }, [user, refreshConversations, refreshThread, refreshAll])
}
