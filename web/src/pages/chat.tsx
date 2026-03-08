import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { LogOut, MessageSquare, Monitor, Moon, Settings, Sun } from 'lucide-react'
import { useCallback, useEffect, useRef } from 'react'
import type { ThemeMode } from '@/lib/theme'

import { ConversationList } from '@/components/conversation-list'
import { KeyboardShortcutsDialog } from '@/components/keyboard-shortcuts-dialog'
import { NewConversation } from '@/components/new-conversation'
import { ThreadView } from '@/components/thread-view'
import { fetchJson, postJson } from '@/lib/api'
import type { ConversationSummary } from '@/lib/types'
import { authAtom } from '@/store/auth'
import { themeAtom } from '@/store/theme'
import {
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
  hasMoreAtom,
  initialLoadingAtom,
  loadingMoreAtom,
  mobileViewAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  shortcutsDialogOpenAtom,
  showArchivedAtom,
} from '@/store/chat'
import { useKeyboardNav } from '@/hooks/use-keyboard-nav'
import { useMailEvents } from '@/hooks/use-mail-events'

const PAGE_SIZE = 50

export function Chat() {
  const auth = useAtomValue(authAtom)
  const composingNew = useAtomValue(composingNewAtom)
  const [conversations, setConversations] = useAtom(conversationsAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const setHasMore = useSetAtom(hasMoreAtom)
  const setLoadingMore = useSetAtom(loadingMoreAtom)
  const setInitialLoading = useSetAtom(initialLoadingAtom)
  const [mobileView, setMobileView] = useAtom(mobileViewAtom)
  const [shortcutsOpen, setShortcutsOpen] = useAtom(shortcutsDialogOpenAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null)
  const firstLoadDone = useRef(false)

  // keep a ref so loadMore always sees latest
  const conversationsRef = useRef(conversations)
  conversationsRef.current = conversations
  const searchRef = useRef(searchQuery)
  searchRef.current = searchQuery
  const categoryRef = useRef(categoryFilter)
  categoryRef.current = categoryFilter
  const domainsRef = useRef(selectedDomains)
  domainsRef.current = selectedDomains
  const archivedRef = useRef(showArchived)
  archivedRef.current = showArchived

  // request notification permission
  useEffect(() => {
    if (Notification.permission === 'default') {
      Notification.requestPermission()
    }
  }, [])

  // websocket events
  useMailEvents(auth?.address ?? '')

  // keyboard navigation
  useKeyboardNav()

  // build API path helper
  const buildPath = useCallback(
    (opts?: { query?: string; before?: number; category?: string | null; domains?: string[]; archived?: boolean }) => {
      const { query, before, category, domains, archived } = opts ?? {}
      if (query) {
        let path = `/conversations/search?q=${encodeURIComponent(query)}&limit=${PAGE_SIZE}`
        if (category) path += `&category=${encodeURIComponent(category)}`
        if (domains && domains.length > 0) path += `&domains=${encodeURIComponent(domains.join(','))}`
        return path
      }
      let path = `/conversations?limit=${PAGE_SIZE}`
      if (before) path += `&before=${before}`
      if (category) path += `&category=${encodeURIComponent(category)}`
      if (domains && domains.length > 0) path += `&domains=${encodeURIComponent(domains.join(','))}`
      if (archived) path += '&archived=true'
      return path
    },
    []
  )

  // load conversations with optional append mode
  const loadConversations = useCallback(
    async (opts?: { query?: string; before?: number; category?: string | null; domains?: string[]; archived?: boolean; append?: boolean }) => {
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
      } finally {
        if (!firstLoadDone.current) {
          firstLoadDone.current = true
          setInitialLoading(false)
        }
      }
    },
    [setConversations, setHasMore, setInitialLoading, buildPath]
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
        domains: domainsRef.current.length > 0 ? domainsRef.current : undefined,
        archived: archivedRef.current || undefined,
        append: true,
      })
    } finally {
      loadingRef.current = false
      setLoadingMore(false)
    }
  }, [setLoadingMore, loadConversations])

  // initial load + react to filter/search/domain/archived changes
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      setHasMore(true)
      loadConversations({
        query: searchQuery || undefined,
        category: categoryFilter,
        domains: selectedDomains.length > 0 ? selectedDomains : undefined,
        archived: showArchived || undefined,
      })
    }, 300)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [searchQuery, categoryFilter, selectedDomains, showArchived, loadConversations, setHasMore])

  const showList = mobileView === 'list'
  const showThread = mobileView === 'thread'

  return (
    <div className="flex h-screen bg-white text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      {/* sidebar: hidden on mobile, visible on md+ */}
      <div className="hidden md:flex">
        <ChatSidebar />
      </div>

      {/* conversation list: full width on mobile when showing list, fixed width on desktop */}
      <div
        className={`${
          showList ? 'flex' : 'hidden'
        } w-full shrink-0 flex-col border-r border-zinc-200 dark:border-zinc-800 md:flex md:w-80`}
      >
        <ConversationList
          onLoadMore={loadMore}
          onSelectConversation={() => setMobileView('thread')}
        />
      </div>

      {/* main content: full width on mobile when showing thread, flex-1 on desktop */}
      <div
        className={`${
          showThread ? 'flex' : 'hidden'
        } min-w-0 flex-1 flex-col md:flex`}
      >
        {composingNew ? <NewConversation /> : <ThreadView onBack={() => setMobileView('list')} />}
      </div>

      <KeyboardShortcutsDialog
        open={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />
    </div>
  )
}

const THEME_CYCLE: ThemeMode[] = ['system', 'light', 'dark']

function ChatSidebar() {
  const auth = useAtomValue(authAtom)
  const setAuth = useSetAtom(authAtom)
  const [theme, setTheme] = useAtom(themeAtom)

  const cycleTheme = () => {
    const idx = THEME_CYCLE.indexOf(theme)
    const next = THEME_CYCLE[(idx + 1) % THEME_CYCLE.length]
    setTheme(next)
  }

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
      <div className="mb-4">
        <img src="/icon.svg" alt="mailrs" className="h-9 w-9 rounded-lg" />
      </div>

      {/* nav icons */}
      <nav className="flex flex-1 flex-col items-center gap-1.5">
        <a
          href="/"
          className="flex h-9 w-9 items-center justify-center rounded-lg bg-blue-50 text-blue-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:bg-blue-950 dark:text-blue-400"
          title="Chat"
          aria-label="Chat"
          aria-current="page"
        >
          <MessageSquare className="h-5 w-5" />
        </a>
        <a
          href="/admin"
          className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title="Admin"
          aria-label="Admin"
        >
          <Settings className="h-5 w-5" />
        </a>
      </nav>

      {/* user */}
      <div className="flex flex-col items-center gap-2">
        {/* theme toggle */}
        <button
          onClick={cycleTheme}
          className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title={`Theme: ${theme}`}
          aria-label={`Switch theme, current: ${theme}`}
        >
          {theme === 'dark' ? (
            <Moon className="h-5 w-5" />
          ) : theme === 'light' ? (
            <Sun className="h-5 w-5" />
          ) : (
            <Monitor className="h-5 w-5" />
          )}
        </button>

        {/* settings */}
        <a
          href="/settings"
          className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title="Settings"
          aria-label="Settings"
        >
          <Settings className="h-5 w-5" />
        </a>

        <button
          onClick={handleLogout}
          className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-600/50 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
          title={`Sign out (${auth?.address})`}
          aria-label={`Sign out (${auth?.address})`}
        >
          <LogOut className="h-5 w-5" />
        </button>
      </div>
    </aside>
  )
}
