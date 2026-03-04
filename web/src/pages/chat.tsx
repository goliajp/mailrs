import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef } from 'react'

import { ConversationList } from '@/components/conversation-list'
import { NewConversation } from '@/components/new-conversation'
import { ThreadView } from '@/components/thread-view'
import { fetchJson, postJson } from '@/lib/api'
import type { ConversationSummary } from '@/lib/types'
import { authAtom } from '@/store/auth'
import {
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
  hasMoreAtom,
  loadingMoreAtom,
  searchQueryAtom,
} from '@/store/chat'
import { useMailEvents } from '@/hooks/use-mail-events'

const PAGE_SIZE = 50

export function Chat() {
  const auth = useAtomValue(authAtom)
  const composingNew = useAtomValue(composingNewAtom)
  const [conversations, setConversations] = useAtom(conversationsAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const setHasMore = useSetAtom(hasMoreAtom)
  const setLoadingMore = useSetAtom(loadingMoreAtom)
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null)

  // keep a ref so loadMore always sees latest
  const conversationsRef = useRef(conversations)
  conversationsRef.current = conversations
  const searchRef = useRef(searchQuery)
  searchRef.current = searchQuery
  const categoryRef = useRef(categoryFilter)
  categoryRef.current = categoryFilter

  // request notification permission
  useEffect(() => {
    if (Notification.permission === 'default') {
      Notification.requestPermission()
    }
  }, [])

  // websocket events
  useMailEvents(auth?.address ?? '')

  // build API path helper
  const buildPath = useCallback(
    (opts?: { query?: string; before?: number; category?: string | null }) => {
      const { query, before, category } = opts ?? {}
      if (query) {
        let path = `/conversations/search?q=${encodeURIComponent(query)}&limit=${PAGE_SIZE}`
        if (category) path += `&category=${encodeURIComponent(category)}`
        return path
      }
      let path = `/conversations?limit=${PAGE_SIZE}`
      if (before) path += `&before=${before}`
      if (category) path += `&category=${encodeURIComponent(category)}`
      return path
    },
    []
  )

  // load conversations with optional append mode
  const loadConversations = useCallback(
    async (opts?: { query?: string; before?: number; category?: string | null; append?: boolean }) => {
      const { append } = opts ?? {}
      try {
        const path = buildPath(opts)
        const data = await fetchJson<ConversationSummary[]>(path)

        if (append) {
          setConversations((prev) => [...prev, ...data])
        } else {
          setConversations(data)
        }

        setHasMore(data.length >= PAGE_SIZE)
      } catch {
        // keep current
      }
    },
    [setConversations, setHasMore, buildPath]
  )

  // load more (infinite scroll callback) with reentry guard
  const loadingRef = useRef(false)
  const loadMore = useCallback(async () => {
    if (loadingRef.current) return
    const current = conversationsRef.current
    const last = current[current.length - 1]
    if (!last) return

    loadingRef.current = true
    setLoadingMore(true)
    try {
      await loadConversations({
        query: searchRef.current || undefined,
        before: last.last_date,
        category: categoryRef.current,
        append: true,
      })
    } finally {
      loadingRef.current = false
      setLoadingMore(false)
    }
  }, [setLoadingMore, loadConversations])

  // initial load + react to filter/search changes
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      setHasMore(true)
      loadConversations({
        query: searchQuery || undefined,
        category: categoryFilter,
      })
    }, 300)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [searchQuery, categoryFilter, loadConversations, setHasMore])

  return (
    <div className="flex h-screen bg-white text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      {/* sidebar */}
      <ChatSidebar />

      {/* conversation list */}
      <div className="flex w-80 shrink-0 flex-col border-r border-zinc-200 dark:border-zinc-800">
        <ConversationList onLoadMore={loadMore} />
      </div>

      {/* main content */}
      {composingNew ? <NewConversation /> : <ThreadView />}
    </div>
  )
}

function ChatSidebar() {
  const auth = useAtomValue(authAtom)
  const setAuth = useSetAtom(authAtom)

  const handleLogout = async () => {
    try {
      await postJson('/auth/logout', {})
    } catch {
      // ignore
    }
    setAuth(null)
    window.location.href = '/login'
  }

  return (
    <aside className="flex h-full w-14 shrink-0 flex-col items-center border-r border-zinc-200 bg-zinc-50 py-4 dark:border-zinc-800 dark:bg-zinc-900/50">
      {/* logo */}
      <div className="mb-4 flex h-9 w-9 items-center justify-center rounded-lg bg-blue-500 text-sm font-bold text-white">
        M
      </div>

      {/* nav icons */}
      <nav className="flex flex-1 flex-col items-center gap-1">
        <a
          href="/"
          className="flex h-9 w-9 items-center justify-center rounded-lg bg-zinc-200 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
          title="Chat"
        >
          <svg
            className="h-5 w-5"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M8.625 12a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H8.25m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H12m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0h-.375M21 12c0 4.556-4.03 8.25-9 8.25a9.764 9.764 0 01-2.555-.337A5.972 5.972 0 015.41 20.97a5.969 5.969 0 01-.474-.065 4.48 4.48 0 00.978-2.025c.09-.457-.133-.901-.467-1.226C3.93 16.178 3 14.189 3 12c0-4.556 4.03-8.25 9-8.25s9 3.694 9 8.25z"
            />
          </svg>
        </a>
        <a
          href="/admin"
          className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-700 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title="Admin"
        >
          <svg
            className="h-5 w-5"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.324.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.24-.438.613-.431.992a6.759 6.759 0 010 .255c-.007.378.138.75.43.99l1.005.828c.424.35.534.954.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.28c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.992a6.932 6.932 0 010-.255c.007-.378-.138-.75-.43-.99l-1.004-.828a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.281z"
            />
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
            />
          </svg>
        </a>
      </nav>

      {/* user */}
      <div className="flex flex-col items-center gap-2">
        <button
          onClick={handleLogout}
          className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-700 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title={`Sign out (${auth?.address})`}
        >
          <svg
            className="h-5 w-5"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15m3 0l3-3m0 0l-3-3m3 3H9"
            />
          </svg>
        </button>
      </div>
    </aside>
  )
}
