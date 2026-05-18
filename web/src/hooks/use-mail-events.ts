import type { ConversationSummary, NewMessageEvent, SmtpEvent, ThreadMessage } from '@/lib/types'

import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef } from 'react'

import { fetchJson } from '@/lib/api'
import { playNotificationSound } from '@/lib/notification-sound'
import {
  categoryFilterAtom,
  connectionStatusAtom,
  conversationsAtom,
  folderAtom,
  importanceSectionAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  threadMessagesAtom,
} from '@/store/chat'
import { notificationsAtom, notificationSoundAtom } from '@/store/settings'

// shallow equality over the conversation fields ConversationItem actually
// renders. Used to preserve object identity across WS refetches so memo'd
// rows don't re-render when their payload is unchanged.
function shallowEqualConvo(a: ConversationSummary, b: ConversationSummary): boolean {
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
  const setConversations = useSetAtom(conversationsAtom)
  const setConnectionStatus = useSetAtom(connectionStatusAtom)
  const setThreadMessages = useSetAtom(threadMessagesAtom)
  const selectedThreadId = useAtomValue(selectedThreadIdAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedRef = useRef(selectedThreadId)
  const categoryRef = useRef(categoryFilter)
  const searchQuery = useAtomValue(searchQueryAtom)
  const searchRef = useRef(searchQuery)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const domainsRef = useRef(selectedDomains)
  const folder = useAtomValue(folderAtom)
  const folderRef = useRef(folder)
  const quickFilter = useAtomValue(quickFilterAtom)
  const quickFilterRef = useRef(quickFilter)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const sectionRef = useRef(importanceSection)
  const notificationsEnabled = useAtomValue(notificationsAtom)
  const notificationsRef = useRef(notificationsEnabled)
  const soundEnabled = useAtomValue(notificationSoundAtom)
  const soundRef = useRef(soundEnabled)

  useEffect(() => {
    selectedRef.current = selectedThreadId
    categoryRef.current = categoryFilter
    searchRef.current = searchQuery
    domainsRef.current = selectedDomains
    folderRef.current = folder
    quickFilterRef.current = quickFilter
    sectionRef.current = importanceSection
    notificationsRef.current = notificationsEnabled
    soundRef.current = soundEnabled
  }, [
    selectedThreadId,
    categoryFilter,
    searchQuery,
    selectedDomains,
    folder,
    quickFilter,
    importanceSection,
    notificationsEnabled,
    soundEnabled,
  ])

  const wsRef = useRef<null | WebSocket>(null)
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
    const qf = quickFilterRef.current
    if (qf === 'unread') path += '&unread=true'
    if (qf === 'starred') path += '&starred=true'
    const sec = sectionRef.current
    if (sec) path += `&section=${encodeURIComponent(sec)}`
    // Replace, don't merge. The previous "keep prev items beyond the fresh
    // window" stitching mixed items from earlier filter contexts into the
    // current list. Server already returns the top-50 for the active
    // filter; trust that.
    //
    // Preserve object identity for items whose payload didn't change, so
    // React.memo on ConversationItem actually skips renders. Without this
    // every WS tick reallocates every visible row.
    fetchJson<ConversationSummary[]>(path).then(
      (fresh) =>
        setConversations((prev) => {
          const byId = new Map<string, ConversationSummary>()
          for (const c of prev) byId.set(c.thread_id, c)
          return fresh.map((f) => {
            const existing = byId.get(f.thread_id)
            return existing && shallowEqualConvo(existing, f) ? existing : f
          })
        }),
      () => {}
    )
  }, [setConversations])

  const refreshThread = useCallback(() => {
    const tid = selectedRef.current
    if (!tid) return
    const doms = domainsRef.current
    const domainsParam = doms.length > 0 ? `?domains=${encodeURIComponent(doms.join(','))}` : ''
    fetchJson<ThreadMessage[]>(`/conversations/${encodeURIComponent(tid)}${domainsParam}`).then(
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
    let reconnectDelay = RECONNECT_BASE

    function connect() {
      if (closed) return
      if (!navigator.onLine) return
      // clear previous ping timer
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
        // send periodic pings to keep connection alive and detect dead sockets
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
              refreshConversations()

              // refresh current thread if the new message belongs to it
              if (selectedRef.current && msg.thread_id === selectedRef.current) {
                refreshThread()
              }

              if (notificationsRef.current) {
                if (soundRef.current) playNotificationSound()

                if (
                  typeof Notification !== 'undefined' &&
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
        refreshAll()
      }, POLL_INTERVAL)
    }
    startPolling()

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

    // reconnect immediately when device comes back online
    function onOnline() {
      reconnectDelay = RECONNECT_BASE
      if (
        wsRef.current &&
        wsRef.current.readyState !== WebSocket.OPEN &&
        wsRef.current.readyState !== WebSocket.CONNECTING
      ) {
        connect()
      }
      refreshAll()
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
  }, [user, refreshConversations, refreshThread, refreshAll, setConnectionStatus])
}
